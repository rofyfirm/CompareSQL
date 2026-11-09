//! Data diff berdasarkan primary key.
//!
//! Pass 1: baca A, simpan hash(key) -> hash(baris) (24 byte per baris).
//! Pass 2: baca B, klasifikasi sama / berubah / hanya di B.
//! Pass 3: baca A lagi hanya bila perlu, ambil baris yang hilang/berubah.
//! Tabel tanpa PK/unique NOT NULL: dibandingkan sebagai multiset baris.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use serde::Serialize;
use xxhash_rust::xxh3::Xxh3;

use crate::model::{DefaultValue, Index, IndexKind, Table};
use crate::progress::Progress;
use crate::row::{Cell, RowSource, RowView};
use crate::{Error, Result};

#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for (i, b) in bytes.iter().take(8).enumerate() {
            self.0 |= (*b as u64) << (i * 8);
        }
    }
    fn write_u128(&mut self, v: u128) {
        self.0 = v as u64;
    }
}

type FastMap<V> = HashMap<u128, V, BuildHasherDefault<IdHasher>>;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum KeyMode {
    PrimaryKey,
    UniqueKey,
    /// Tanpa key: bandingkan seluruh baris (multiset).
    FullRow,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangedRow {
    pub a: Vec<Cell>,
    pub b: Vec<Cell>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TableDataDiff {
    pub table: String,
    /// Kolom terpadu (kolom non-generated di B), urutan B.
    pub columns: Vec<String>,
    pub key_columns: Vec<usize>,
    pub key_mode: Option<KeyMode>,
    pub rows_a: u64,
    pub rows_b: u64,
    pub same: u64,
    #[serde(skip)]
    pub only_in_a: Vec<Vec<Cell>>,
    #[serde(skip)]
    pub only_in_b: Vec<Vec<Cell>>,
    #[serde(skip)]
    pub changed: Vec<ChangedRow>,
    /// Kolom terpadu yang tidak ada di A (nilai A dianggap default B).
    pub missing_in_a: Vec<usize>,
    /// Kolom terpadu yang tidak dibandingkan (default B berupa ekspresi).
    pub ignored_columns: Vec<usize>,
    pub warnings: Vec<String>,
}

impl TableDataDiff {
    pub fn is_same(&self) -> bool {
        self.only_in_a.is_empty() && self.only_in_b.is_empty() && self.changed.is_empty()
    }
}

/// Sumber nilai A untuk satu kolom terpadu.
#[derive(Clone)]
enum ASrc {
    Col(usize),
    Const(Cell),
    /// Default B tidak bisa dihitung (ekspresi) -> kolom tidak dibandingkan.
    Unknown,
}

fn default_cell(d: &Option<DefaultValue>, nullable: bool) -> ASrc {
    match d {
        None if nullable => ASrc::Const(None),
        None => ASrc::Unknown,
        Some(DefaultValue::Null) => ASrc::Const(None),
        Some(DefaultValue::Str(s)) => ASrc::Const(Some(s.clone())),
        Some(DefaultValue::Num(n)) => ASrc::Const(Some(n.clone().into_bytes())),
        Some(DefaultValue::Hex(h)) => ASrc::Const(Some(h.clone())),
        Some(DefaultValue::Bits(b)) => {
            let v = u128::from_str_radix(b, 2).unwrap_or(0);
            ASrc::Const(Some(v.to_string().into_bytes()))
        }
        _ => ASrc::Unknown,
    }
}

fn hash_cell(h: &mut Xxh3, c: Option<&[u8]>) {
    match c {
        None => h.update(&[0]),
        Some(b) => {
            h.update(&[1]);
            h.update(&(b.len() as u32).to_le_bytes());
            h.update(b);
        }
    }
}

struct Plan {
    columns: Vec<String>,
    b_idx: Vec<usize>,
    a_src: Vec<ASrc>,
    key: Vec<usize>,
    mode: KeyMode,
    compare: Vec<usize>,
    missing_in_a: Vec<usize>,
    warnings: Vec<String>,
}

fn choose_key(ta: &Table, tb: &Table, plan_cols: &[String], a_src: &[ASrc]) -> (Vec<usize>, KeyMode) {
    let usable = |idx: &Index| -> Option<Vec<usize>> {
        let mut v = vec![];
        for p in &idx.parts {
            let name = p.column.as_ref()?;
            if p.prefix.is_some() {
                return None;
            }
            let pos = plan_cols.iter().position(|c| c.eq_ignore_ascii_case(name))?;
            if !matches!(a_src[pos], ASrc::Col(_)) {
                return None;
            }
            v.push(pos);
        }
        Some(v)
    };
    if let Some(pk) = &tb.primary_key {
        if let Some(v) = usable(pk) {
            return (v, KeyMode::PrimaryKey);
        }
    }
    if let Some(pk) = &ta.primary_key {
        if let Some(v) = usable(pk) {
            return (v, KeyMode::PrimaryKey);
        }
    }
    for idx in &tb.indexes {
        if idx.kind != IndexKind::Unique {
            continue;
        }
        let all_not_null = idx.parts.iter().all(|p| {
            p.column.as_ref().and_then(|n| tb.column(n)).is_some_and(|c| !c.nullable)
        });
        if all_not_null {
            if let Some(v) = usable(idx) {
                return (v, KeyMode::UniqueKey);
            }
        }
    }
    (vec![], KeyMode::FullRow)
}

fn make_plan(ta: &Table, tb: &Table) -> Plan {
    let mut columns = vec![];
    let mut b_idx = vec![];
    let mut a_src = vec![];
    let mut missing_in_a = vec![];
    let mut warnings = vec![];
    for (bi, cb) in tb.columns.iter().enumerate() {
        if cb.generated.is_some() {
            continue;
        }
        let ui = columns.len();
        columns.push(cb.name.clone());
        b_idx.push(bi);
        match ta.columns.iter().position(|c| c.name.eq_ignore_ascii_case(&cb.name) && c.generated.is_none()) {
            Some(ai) => a_src.push(ASrc::Col(ai)),
            None => {
                missing_in_a.push(ui);
                let src = default_cell(&cb.default, cb.nullable);
                if matches!(src, ASrc::Unknown) {
                    warnings.push(format!(
                        "kolom baru `{}` tanpa default literal: nilainya tidak dibandingkan",
                        cb.name
                    ));
                }
                a_src.push(src);
            }
        }
    }
    let (key, mode) = choose_key(ta, tb, &columns, &a_src);
    if mode == KeyMode::FullRow {
        warnings.push("tidak ada primary key / unique NOT NULL: dibandingkan per baris utuh".into());
    }
    let compare = (0..columns.len()).filter(|&i| !matches!(a_src[i], ASrc::Unknown)).collect();
    Plan { columns, b_idx, a_src, key, mode, compare, missing_in_a, warnings }
}

impl Plan {
    fn a_cell<'r>(&'r self, row: &RowView<'r>, ui: usize) -> Option<&'r [u8]> {
        match &self.a_src[ui] {
            ASrc::Col(ai) => row.get(*ai),
            ASrc::Const(c) => c.as_deref(),
            ASrc::Unknown => None,
        }
    }
    fn hashes_a(&self, row: &RowView<'_>) -> (u128, u64) {
        let mut kh = Xxh3::new();
        for &k in &self.key {
            hash_cell(&mut kh, self.a_cell(row, k));
        }
        let mut rh = Xxh3::new();
        for &c in &self.compare {
            hash_cell(&mut rh, self.a_cell(row, c));
        }
        let r128 = rh.digest128();
        if self.mode == KeyMode::FullRow {
            (r128, r128 as u64)
        } else {
            (kh.digest128(), r128 as u64)
        }
    }
    fn hashes_b(&self, row: &RowView<'_>) -> (u128, u64) {
        let mut kh = Xxh3::new();
        for &k in &self.key {
            hash_cell(&mut kh, row.get(self.b_idx[k]));
        }
        let mut rh = Xxh3::new();
        for &c in &self.compare {
            hash_cell(&mut rh, row.get(self.b_idx[c]));
        }
        let r128 = rh.digest128();
        if self.mode == KeyMode::FullRow {
            (r128, r128 as u64)
        } else {
            (kh.digest128(), r128 as u64)
        }
    }
    fn own_a(&self, row: &RowView<'_>) -> Vec<Cell> {
        (0..self.columns.len()).map(|i| self.a_cell(row, i).map(|b| b.to_vec())).collect()
    }
    fn own_b(&self, row: &RowView<'_>) -> Vec<Cell> {
        self.b_idx.iter().map(|&bi| row.get(bi).map(|b| b.to_vec())).collect()
    }
}

const UNSEEN: u8 = 0;
const SAME: u8 = 1;
const CHANGED: u8 = 2;

struct Entry {
    row_hash: u64,
    /// keyed: UNSEEN/SAME/CHANGED. full-row: dipakai sebagai sisa hitungan.
    state: u32,
}

/// Bandingkan data satu tabel. `ta` = schema A, `tb` = schema B.
pub fn diff_table_data(
    a: &mut dyn RowSource,
    b: &mut dyn RowSource,
    ta: &Table,
    tb: &Table,
    progress: &dyn Progress,
) -> Result<TableDataDiff> {
    let plan = make_plan(ta, tb);
    let mut out = TableDataDiff {
        table: tb.name.clone(),
        columns: plan.columns.clone(),
        key_columns: plan.key.clone(),
        key_mode: Some(plan.mode),
        missing_in_a: plan.missing_in_a.clone(),
        ignored_columns: (0..plan.columns.len()).filter(|i| !plan.compare.contains(i)).collect(),
        warnings: plan.warnings.clone(),
        ..Default::default()
    };
    let full_row = plan.mode == KeyMode::FullRow;
    let phase = format!("Data `{}`", tb.name);

    // Pass 1: A
    let mut map: FastMap<Entry> = FastMap::default();
    let mut dup_a = 0u64;
    let mut n = 0u64;
    a.for_each_row(ta, &mut |row| {
        n += 1;
        if n.is_multiple_of(50_000) {
            progress.check()?;
            progress.report(&phase, "membaca A", n, 0);
        }
        let (k, h) = plan.hashes_a(row);
        if full_row {
            map.entry(k).or_insert(Entry { row_hash: h, state: 0 }).state += 1;
        } else if map.insert(k, Entry { row_hash: h, state: UNSEEN as u32 }).is_some() {
            dup_a += 1;
        }
        Ok(())
    })?;
    out.rows_a = n;
    if dup_a > 0 {
        out.warnings.push(format!("{dup_a} key duplikat di A"));
    }

    // Pass 2: B
    let mut changed_b: FastMap<Vec<Cell>> = FastMap::default();
    let mut dup_b = 0u64;
    let mut n = 0u64;
    let mut only_b = vec![];
    let mut same = 0u64;
    b.for_each_row(tb, &mut |row| {
        n += 1;
        if n.is_multiple_of(50_000) {
            progress.check()?;
            progress.report(&phase, "membaca B", n, out.rows_a);
        }
        let (k, h) = plan.hashes_b(row);
        match map.get_mut(&k) {
            Some(e) if full_row => {
                if e.state > 0 {
                    e.state -= 1;
                    same += 1;
                } else {
                    only_b.push(plan.own_b(row));
                }
            }
            Some(e) => {
                if e.state != UNSEEN as u32 {
                    dup_b += 1;
                } else if e.row_hash == h {
                    e.state = SAME as u32;
                    same += 1;
                } else {
                    e.state = CHANGED as u32;
                    changed_b.insert(k, plan.own_b(row));
                }
            }
            None => only_b.push(plan.own_b(row)),
        }
        Ok(())
    })?;
    out.rows_b = n;
    out.same = same;
    out.only_in_b = only_b;
    if dup_b > 0 {
        out.warnings.push(format!("{dup_b} key duplikat di B"));
    }

    // Pass 3: ambil baris A yang hilang / berubah
    let need_a = map.values().any(|e| if full_row { e.state > 0 } else { e.state != SAME as u32 });
    if need_a {
        let mut only_a = vec![];
        let mut changed = vec![];
        a.for_each_row(ta, &mut |row| {
            let (k, _) = plan.hashes_a(row);
            if let Some(e) = map.get_mut(&k) {
                if full_row {
                    if e.state > 0 {
                        e.state -= 1;
                        only_a.push(plan.own_a(row));
                    }
                } else if e.state == UNSEEN as u32 {
                    e.state = SAME as u32; // tandai sudah diambil (key duplikat)
                    only_a.push(plan.own_a(row));
                } else if e.state == CHANGED as u32 {
                    if let Some(bv) = changed_b.remove(&k) {
                        changed.push(ChangedRow { a: plan.own_a(row), b: bv });
                    }
                }
            }
            Ok(())
        })?;
        out.only_in_a = only_a;
        out.changed = changed;
    }
    Ok(out)
}

/// Kolom yang nilainya beda pada baris berubah (indeks kolom terpadu).
pub fn changed_columns(d: &TableDataDiff, row: &ChangedRow) -> Vec<usize> {
    row.a
        .iter()
        .zip(&row.b)
        .enumerate()
        .filter(|(i, (x, y))| x != y && !d.ignored_columns.contains(i))
        .map(|(i, _)| i)
        .collect()
}

pub fn err_if_cancelled(p: &dyn Progress) -> Result<()> {
    if p.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
