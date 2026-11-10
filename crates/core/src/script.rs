//! Generator script sinkronisasi: jalankan di A supaya A sama dengan B.
//!
//! Urutan fase supaya aman terhadap foreign key:
//! 1. DROP FOREIGN KEY yang hilang/berubah/terdampak
//! 2. DROP TABLE (opsional)
//! 3. CREATE TABLE baru (tanpa FK), urut topologis
//! 4. ALTER TABLE (kolom, index, PK, check, opsi)
//! 5. DELETE baris (urut topologis terbalik: anak dulu)
//! 6. UPDATE + INSERT (urut topologis: induk dulu)
//! 7. ADD FOREIGN KEY

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{self, Write};

use serde::{Deserialize, Serialize};

use crate::data_diff::{changed_columns, KeyMode, TableDataDiff};
use crate::model::*;
use crate::normalize::CompareOptions;
use crate::render::*;
use crate::row::Cell;
use crate::schema_diff::*;
use crate::value::write_literal;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ScriptOptions {
    pub include_schema: bool,
    pub include_data: bool,
    pub drop_tables: bool,
    pub drop_columns: bool,
    pub delete_rows: bool,
    pub disable_fk_checks: bool,
    /// Nama tabel (huruf kecil) yang ikut; None = semua.
    pub tables: Option<Vec<String>>,
    pub batch_rows: usize,
    pub max_statement_bytes: usize,
}

impl Default for ScriptOptions {
    fn default() -> Self {
        ScriptOptions {
            include_schema: true,
            include_data: true,
            drop_tables: false,
            drop_columns: true,
            delete_rows: true,
            disable_fk_checks: false,
            tables: None,
            batch_rows: 500,
            max_statement_bytes: 1 << 20,
        }
    }
}

pub struct ScriptInput<'a> {
    pub schema_a: &'a Schema,
    pub schema_b: &'a Schema,
    pub data: &'a [TableDataDiff],
    pub compare: &'a CompareOptions,
    pub header: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub sql: String,
    pub statements: usize,
    pub warnings: Vec<String>,
}

/// Hasil [`generate_to`].
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScriptStats {
    pub statements: usize,
    pub bytes: u64,
    pub warnings: Vec<String>,
    /// false bila berhenti karena `limit` (script lebih panjang dari yang ditulis).
    pub complete: bool,
}

/// Penulis script ke sink apa pun (memori, file, atau hanya penghitung).
/// Setelah `stopped`, semua tulisan berikutnya diabaikan dan loop besar
/// memeriksa `stopped()` untuk keluar lebih awal.
struct W<'o> {
    out: &'o mut dyn Write,
    bytes: u64,
    n: usize,
    warnings: Vec<String>,
    /// Berhenti setelah statement yang membuat total >= limit.
    limit: u64,
    cancel: &'o dyn Fn() -> bool,
    stopped: bool,
    cancelled: bool,
    err: Option<io::Error>,
}

impl W<'_> {
    fn raw(&mut self, s: &str) {
        if self.stopped {
            return;
        }
        if let Err(e) = self.out.write_all(s.as_bytes()) {
            self.err = Some(e);
            self.stopped = true;
            return;
        }
        self.bytes += s.len() as u64;
    }
    fn emit(&mut self, stmt: &str) {
        if self.stopped {
            return;
        }
        self.raw(stmt);
        self.raw(";\n");
        self.n += 1;
        if self.bytes >= self.limit {
            self.stopped = true;
        } else if self.n.is_multiple_of(1024) {
            self.check_cancel();
        }
    }
    fn check_cancel(&mut self) {
        if !self.stopped && (self.cancel)() {
            self.cancelled = true;
            self.stopped = true;
        }
    }
    fn section(&mut self, title: &str) {
        self.check_cancel();
        self.raw(&format!("\n-- {title}\n"));
    }
    fn note(&mut self, text: &str) {
        self.raw(&format!("-- {text}\n"));
    }
    fn stopped(&self) -> bool {
        self.stopped
    }
}

fn lc(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Urutan topologis (induk dulu). `cycle` = true bila ada siklus FK.
pub fn topo_order(tables: &[&Table]) -> (Vec<String>, bool) {
    let names: HashSet<String> = tables.iter().map(|t| lc(&t.name)).collect();
    let mut indeg: HashMap<String, usize> = names.iter().map(|n| (n.clone(), 0)).collect();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for t in tables {
        let child = lc(&t.name);
        let parents: BTreeSet<String> = t
            .foreign_keys
            .iter()
            .map(|f| lc(&f.ref_table))
            .filter(|p| names.contains(p) && *p != child)
            .collect();
        for p in parents {
            *indeg.get_mut(&child).unwrap() += 1;
            children.entry(p).or_default().push(child.clone());
        }
    }
    let mut ready: BTreeSet<String> = indeg.iter().filter(|(_, d)| **d == 0).map(|(n, _)| n.clone()).collect();
    let mut out = vec![];
    while let Some(n) = ready.pop_first() {
        out.push(n.clone());
        if let Some(cs) = children.get(&n) {
            for c in cs {
                let d = indeg.get_mut(c).unwrap();
                *d -= 1;
                if *d == 0 {
                    ready.insert(c.clone());
                }
            }
        }
    }
    let cycle = out.len() < names.len();
    if cycle {
        let mut rest: Vec<String> = names.into_iter().filter(|n| !out.contains(n)).collect();
        rest.sort();
        out.extend(rest);
    }
    (out, cycle)
}

/// Buat seluruh script di memori.
pub fn generate(input: &ScriptInput, opts: &ScriptOptions) -> Script {
    let mut buf = Vec::new();
    let st = generate_to(input, opts, &mut buf, None, &|| false).expect("menulis ke Vec tidak gagal");
    Script { sql: String::from_utf8(buf).expect("script selalu UTF-8"), statements: st.statements, warnings: st.warnings }
}

/// Tulis script ke `out` secara streaming, tanpa menahan seluruh script di memori.
///
/// `limit`: berhenti setelah statement yang membuat total byte >= limit
/// (untuk pratinjau; `complete` = false). `cancel` dicek di awal tiap bagian
/// dan tiap 1024 statement; bila true, hasilnya `Error::Cancelled`.
pub fn generate_to(
    input: &ScriptInput,
    opts: &ScriptOptions,
    out: &mut dyn Write,
    limit: Option<u64>,
    cancel: &dyn Fn() -> bool,
) -> crate::Result<ScriptStats> {
    let mut w = W {
        out,
        bytes: 0,
        n: 0,
        warnings: vec![],
        limit: limit.unwrap_or(u64::MAX),
        cancel,
        stopped: false,
        cancelled: false,
        err: None,
    };
    write_script(&mut w, input, opts);
    if let Some(e) = w.err {
        return Err(e.into());
    }
    if w.cancelled {
        return Err(crate::Error::Cancelled);
    }
    Ok(ScriptStats { statements: w.n, bytes: w.bytes, warnings: w.warnings, complete: !w.stopped })
}

fn write_script(w: &mut W, input: &ScriptInput, opts: &ScriptOptions) {
    let cmp = input.compare;
    let selected: Option<HashSet<&str>> = opts.tables.as_ref().map(|v| v.iter().map(String::as_str).collect());
    let selected = |name: &str| selected.as_ref().is_none_or(|s| s.contains(lc(name).as_str()));
    let pairs: Vec<(Option<&Table>, Option<&Table>)> = pair_tables(input.schema_a, input.schema_b, cmp)
        .into_iter()
        .filter(|(a, b)| selected(&a.or(*b).unwrap().name))
        .collect();

    for h in &input.header {
        w.note(h);
    }
    w.note("Catatan: DDL MySQL melakukan commit implisit; jalankan pada salinan atau setelah backup.");
    w.raw("\n");
    w.emit("SET NAMES utf8mb4");
    w.emit("SET @OLD_TIME_ZONE=@@TIME_ZONE");
    w.emit("SET TIME_ZONE='+00:00'");
    w.emit("SET @OLD_SQL_MODE=@@SQL_MODE");
    w.emit("SET SQL_MODE='NO_AUTO_VALUE_ON_ZERO'");
    if opts.disable_fk_checks {
        w.emit("SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS");
        w.emit("SET FOREIGN_KEY_CHECKS=0");
    }

    // Tabel B untuk urutan topologis.
    let b_tables: Vec<&Table> = input.schema_b.tables.iter().collect();
    let (order_b, cycle) = topo_order(&b_tables);
    if cycle && !opts.disable_fk_checks {
        w.warnings.push("Ada siklus foreign key; urutan data mungkin gagal. Aktifkan 'nonaktifkan FK checks'.".into());
    }
    let rank: HashMap<String, usize> = order_b.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
    let rank_of = |name: &str| rank.get(&lc(name)).copied().unwrap_or(usize::MAX);

    // Tabel/kolom yang strukturnya berubah -> FK terkait di-drop & dibuat ulang.
    let mut touched_tables: HashSet<String> = HashSet::new();
    let mut touched_cols: HashSet<(String, String)> = HashSet::new();
    if opts.include_schema {
        for (ta, tb) in &pairs {
            if let (Some(ta), Some(tb)) = (ta, tb) {
                let pk_changed = match (&ta.primary_key, &tb.primary_key) {
                    (Some(x), Some(y)) => !index_eq(x, y, cmp),
                    (None, None) => false,
                    _ => true,
                };
                let idx_changed = ta.indexes.iter().any(|ia| {
                    tb.indexes.iter().find(|ib| ib.name.eq_ignore_ascii_case(&ia.name)).is_none_or(|ib| !index_eq(ia, ib, cmp))
                });
                if pk_changed || idx_changed {
                    touched_tables.insert(lc(&tb.name));
                }
                for cb in &tb.columns {
                    if let Some(ca) = ta.column(&cb.name) {
                        if !column_diffs(ca, cb, cmp).is_empty() {
                            touched_cols.insert((lc(&tb.name), lc(&cb.name)));
                        }
                    }
                }
                for ca in &ta.columns {
                    if tb.column(&ca.name).is_none() {
                        touched_cols.insert((lc(&tb.name), lc(&ca.name)));
                    }
                }
            }
        }
    }
    let fk_touched = |child: &str, fk: &ForeignKey| {
        touched_tables.contains(&lc(child))
            || touched_tables.contains(&lc(&fk.ref_table))
            || fk.columns.iter().any(|c| touched_cols.contains(&(lc(child), lc(c))))
            || fk.ref_columns.iter().any(|c| touched_cols.contains(&(lc(&fk.ref_table), lc(c))))
    };

    // FK yang perlu di-drop (di A) dan ditambah (dari B).
    let mut drop_fks: Vec<(String, String)> = vec![];
    let mut add_fks: Vec<(String, ForeignKey)> = vec![];
    if opts.include_schema {
        for (ta, tb) in &pairs {
            match (ta, tb) {
                (Some(ta), Some(tb)) => {
                    let (matched, only_a, only_b) = match_fks(&ta.foreign_keys, &tb.foreign_keys, cmp);
                    for (fa, fb) in matched {
                        if !fk_def_eq(fa, fb, cmp) || fk_touched(&tb.name, fb) {
                            drop_fks.push((ta.name.clone(), fa.name.clone()));
                            add_fks.push((ta.name.clone(), fb.clone()));
                        }
                    }
                    for fa in only_a {
                        drop_fks.push((ta.name.clone(), fa.name.clone()));
                    }
                    for fb in only_b {
                        add_fks.push((ta.name.clone(), fb.clone()));
                    }
                }
                (Some(ta), None) if opts.drop_tables => {
                    for fa in &ta.foreign_keys {
                        drop_fks.push((ta.name.clone(), fa.name.clone()));
                    }
                }
                (None, Some(tb)) => {
                    for fb in &tb.foreign_keys {
                        add_fks.push((tb.name.clone(), fb.clone()));
                    }
                }
                _ => {}
            }
        }
        // FK di tabel lain (tidak dipilih) yang merujuk tabel terdampak tidak ditangani.
    }

    let mut sorted_pairs = pairs.clone();
    sorted_pairs.sort_by_key(|(a, b)| rank_of(&b.or(*a).unwrap().name));

    if opts.include_schema {
        if !drop_fks.is_empty() {
            w.section("1. Hapus foreign key");
            let mut by_table: Vec<(String, Vec<String>)> = vec![];
            for (t, f) in &drop_fks {
                match by_table.iter_mut().find(|(x, _)| x == t) {
                    Some((_, v)) => v.push(f.clone()),
                    None => by_table.push((t.clone(), vec![f.clone()])),
                }
            }
            for (t, fks) in by_table {
                let clauses: Vec<String> = fks.iter().map(|f| format!("DROP FOREIGN KEY {}", q(f))).collect();
                w.emit(&format!("ALTER TABLE {} {}", q(&t), clauses.join(", ")));
            }
        }

        let only_a: Vec<&Table> = pairs.iter().filter_map(|(a, b)| if b.is_none() { *a } else { None }).collect();
        if !only_a.is_empty() {
            w.section("2. Tabel yang hanya ada di A");
            if opts.drop_tables {
                let (order_a, _) = topo_order(&only_a);
                for name in order_a.iter().rev() {
                    let t = only_a.iter().find(|t| lc(&t.name) == *name).unwrap();
                    w.emit(&format!("DROP TABLE {}", q(&t.name)));
                }
            } else {
                for t in &only_a {
                    w.note(&format!("tabel {} tidak di-drop (opsi 'drop tabel' mati)", q(&t.name)));
                }
            }
        }

        let creates: Vec<&Table> =
            sorted_pairs.iter().filter_map(|(a, b)| if a.is_none() { *b } else { None }).collect();
        if !creates.is_empty() {
            w.section("3. Buat tabel baru");
            for t in creates {
                w.emit(&render_create_table(t, false, false));
            }
        }

        let mut alters = vec![];
        for (ta, tb) in &sorted_pairs {
            if let (Some(ta), Some(tb)) = (ta, tb) {
                if let Some(stmt) = alter_table(ta, tb, cmp, opts, &mut w.warnings) {
                    alters.push(stmt);
                }
            }
        }
        if !alters.is_empty() {
            w.section("4. Ubah struktur tabel");
            for s in alters {
                w.emit(&s);
            }
        }
    }

    if opts.include_data {
        let data: HashMap<String, &TableDataDiff> = input.data.iter().map(|d| (lc(&d.table), d)).collect();
        // Nama target di A untuk tiap tabel B.
        let target_name: HashMap<String, (String, &Table)> = pairs
            .iter()
            .filter_map(|(a, b)| b.map(|tb| (lc(&tb.name), (a.unwrap_or(tb).name.clone(), tb))))
            .collect();
        let mut ordered: Vec<&TableDataDiff> = data
            .iter()
            .filter(|(k, d)| target_name.contains_key(*k) && !d.is_same())
            .map(|(_, d)| *d)
            .collect();
        ordered.sort_by_key(|d| rank_of(&d.table));

        if opts.delete_rows && ordered.iter().any(|d| !d.only_in_a.is_empty()) {
            w.section("5. Hapus baris (anak dulu)");
            for d in ordered.iter().rev() {
                let (name, tb) = &target_name[&lc(&d.table)];
                write_deletes(w, name, tb, d, opts);
            }
        }
        if ordered.iter().any(|d| !d.only_in_b.is_empty() || !d.changed.is_empty()) {
            w.section("6. Ubah dan tambah baris (induk dulu)");
            for d in &ordered {
                let (name, tb) = &target_name[&lc(&d.table)];
                write_updates(w, name, tb, d);
                write_inserts(w, name, tb, d, opts);
            }
        }
    }

    if opts.include_schema && !add_fks.is_empty() {
        w.section("7. Tambah foreign key");
        add_fks.sort_by_key(|(t, _)| rank_of(t));
        let mut by_table: Vec<(String, Vec<ForeignKey>)> = vec![];
        for (t, f) in add_fks {
            match by_table.iter_mut().find(|(x, _)| *x == t) {
                Some((_, v)) => v.push(f),
                None => by_table.push((t, vec![f])),
            }
        }
        for (t, fks) in by_table {
            let clauses: Vec<String> = fks.iter().map(|f| format!("ADD {}", render_fk(f))).collect();
            w.emit(&format!("ALTER TABLE {}\n  {}", q(&t), clauses.join(",\n  ")));
        }
    }

    w.raw("\n");
    if opts.disable_fk_checks {
        w.emit("SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS");
    }
    w.emit("SET SQL_MODE=@OLD_SQL_MODE");
    w.emit("SET TIME_ZONE=@OLD_TIME_ZONE");
}

fn position(prev: Option<&str>) -> String {
    match prev {
        None => " FIRST".into(),
        Some(p) => format!(" AFTER {}", q(p)),
    }
}

/// ALTER TABLE untuk satu pasangan; None bila tidak ada perubahan.
fn alter_table(ta: &Table, tb: &Table, cmp: &CompareOptions, opts: &ScriptOptions, warnings: &mut Vec<String>) -> Option<String> {
    let mut c: Vec<String> = vec![];
    let tname = &ta.name;

    // a) CHECK yang hilang
    let (chk_a, chk_b) = match_checks(&ta.checks, &tb.checks);
    for ch in chk_a {
        match &ch.name {
            Some(n) => c.push(format!("DROP CONSTRAINT {}", q(n))),
            None => warnings.push(format!("{tname}: CHECK tanpa nama tidak bisa di-drop: {}", ch.expr)),
        }
    }
    // b) index & PK yang hilang/berubah
    let pk_changed = match (&ta.primary_key, &tb.primary_key) {
        (Some(x), Some(y)) => !index_eq(x, y, cmp),
        (Some(_), None) => true,
        _ => false,
    };
    if pk_changed {
        c.push("DROP PRIMARY KEY".into());
    }
    let mut add_idx: Vec<&Index> = vec![];
    for ia in &ta.indexes {
        match tb.indexes.iter().find(|ib| ib.name.eq_ignore_ascii_case(&ia.name)) {
            None => c.push(format!("DROP INDEX {}", q(&ia.name))),
            Some(ib) if !index_eq(ia, ib, cmp) => {
                c.push(format!("DROP INDEX {}", q(&ia.name)));
                add_idx.push(ib);
            }
            _ => {}
        }
    }
    for ib in &tb.indexes {
        if !ta.indexes.iter().any(|ia| ia.name.eq_ignore_ascii_case(&ib.name)) {
            add_idx.push(ib);
        }
    }
    // c) kolom yang hilang
    for ca in &ta.columns {
        if tb.column(&ca.name).is_none() {
            if opts.drop_columns {
                c.push(format!("DROP COLUMN {}", q(&ca.name)));
                warnings.push(format!("{tname}: kolom {} di-drop, datanya hilang", q(&ca.name)));
            } else {
                warnings.push(format!("{tname}: kolom {} hanya di A, tidak di-drop", q(&ca.name)));
            }
        }
    }
    // d) kolom baru / berubah / pindah, urut B
    let moved: HashSet<String> =
        if cmp.ignore_column_order { HashSet::new() } else { moved_columns(ta, tb).into_iter().map(|s| lc(&s)).collect() };
    let mut prev: Option<&str> = None;
    for cb in &tb.columns {
        match ta.column(&cb.name) {
            None => c.push(format!("ADD COLUMN {}{}", render_column(cb), position(prev))),
            Some(ca) => {
                let changed = !column_diffs(ca, cb, cmp).is_empty();
                let mv = moved.contains(&lc(&cb.name));
                if changed || mv {
                    let mut s = format!("MODIFY COLUMN {}", render_column(cb));
                    if mv {
                        s.push_str(&position(prev));
                    }
                    c.push(s);
                }
            }
        }
        prev = Some(&cb.name);
    }
    // e) PK & index baru
    if let Some(pk) = &tb.primary_key {
        if pk_changed || ta.primary_key.is_none() {
            c.push(format!("ADD {}", render_index(pk)));
        }
    }
    for ib in add_idx {
        c.push(format!("ADD {}", render_index(ib)));
    }
    // f) CHECK baru
    for ch in chk_b {
        c.push(format!("ADD {}", render_check(ch)));
    }
    // g) opsi tabel
    let o_changes = option_diffs(&ta.options, &tb.options, cmp);
    for ch in o_changes {
        let o = &tb.options;
        match ch.name.as_str() {
            "ENGINE" => {
                if let Some(e) = &o.engine {
                    c.push(format!("ENGINE={e}"));
                }
            }
            "CHARSET" | "COLLATE" => {
                if let Some(cs) = &o.charset {
                    let mut s = format!("DEFAULT CHARSET={cs}");
                    if let Some(co) = &o.collation {
                        s.push_str(&format!(" COLLATE={co}"));
                    }
                    c.push(s);
                }
            }
            "COMMENT" => c.push(format!("COMMENT={}", crate::lexer::quote_str(o.comment.as_deref().unwrap_or("").as_bytes()))),
            "ROW_FORMAT" => c.push(format!("ROW_FORMAT={}", o.row_format.as_deref().unwrap_or("DEFAULT"))),
            "AUTO_INCREMENT" => {
                if let Some(n) = o.auto_increment {
                    c.push(format!("AUTO_INCREMENT={n}"));
                }
            }
            "PARTITION" => warnings.push(format!("{tname}: perubahan PARTITION tidak dibuat otomatis")),
            k => match o.other.get(k) {
                Some(v) if !v.is_empty() => c.push(format!("{k}={v}")),
                _ => warnings.push(format!("{tname}: opsi {k} berubah, tidak dibuat otomatis")),
            },
        }
    }
    if c.is_empty() {
        None
    } else {
        Some(format!("ALTER TABLE {}\n  {}", q(tname), c.join(",\n  ")))
    }
}

fn classes(tb: &Table, d: &TableDataDiff) -> Vec<TypeClass> {
    d.columns
        .iter()
        .map(|n| tb.column(n).map(|c| type_class(&c.data_type)).unwrap_or(TypeClass::Other))
        .collect()
}

fn where_key(d: &TableDataDiff, cls: &[TypeClass], row: &[Cell]) -> String {
    let parts: Vec<String> = d
        .key_columns
        .iter()
        .map(|&k| {
            let mut s = format!("{}=", q(&d.columns[k]));
            write_literal(&mut s, row[k].as_deref(), cls[k]);
            s
        })
        .collect();
    parts.join(" AND ")
}

fn write_deletes(w: &mut W, name: &str, tb: &Table, d: &TableDataDiff, opts: &ScriptOptions) {
    if d.only_in_a.is_empty() || w.stopped() {
        return;
    }
    let cls = classes(tb, d);
    w.note(&format!("{}: hapus {} baris", q(name), d.only_in_a.len()));
    if d.key_mode == Some(KeyMode::FullRow) {
        let cols: Vec<usize> = (0..d.columns.len()).filter(|i| !d.missing_in_a.contains(i)).collect();
        for row in &d.only_in_a {
            if w.stopped() {
                return;
            }
            let conds: Vec<String> = cols
                .iter()
                .map(|&i| {
                    let mut s = format!("{} <=> ", q(&d.columns[i]));
                    write_literal(&mut s, row[i].as_deref(), cls[i]);
                    s
                })
                .collect();
            w.emit(&format!("DELETE FROM {} WHERE {} LIMIT 1", q(name), conds.join(" AND ")));
        }
        return;
    }
    let single = d.key_columns.len() == 1;
    let head = if single {
        format!("DELETE FROM {} WHERE {} IN (", q(name), q(&d.columns[d.key_columns[0]]))
    } else {
        let cols: Vec<String> = d.key_columns.iter().map(|&k| q(&d.columns[k])).collect();
        format!("DELETE FROM {} WHERE ({}) IN (", q(name), cols.join(","))
    };
    for chunk in d.only_in_a.chunks(opts.batch_rows.max(1)) {
        if w.stopped() {
            return;
        }
        let mut s = head.clone();
        for (i, row) in chunk.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            if single {
                let k = d.key_columns[0];
                write_literal(&mut s, row[k].as_deref(), cls[k]);
            } else {
                s.push('(');
                for (j, &k) in d.key_columns.iter().enumerate() {
                    if j > 0 {
                        s.push(',');
                    }
                    write_literal(&mut s, row[k].as_deref(), cls[k]);
                }
                s.push(')');
            }
        }
        s.push(')');
        w.emit(&s);
    }
}

fn write_updates(w: &mut W, name: &str, tb: &Table, d: &TableDataDiff) {
    if d.changed.is_empty() || w.stopped() {
        return;
    }
    let cls = classes(tb, d);
    w.note(&format!("{}: ubah {} baris", q(name), d.changed.len()));
    for row in &d.changed {
        if w.stopped() {
            return;
        }
        let cols = changed_columns(d, row);
        if cols.is_empty() {
            continue;
        }
        let sets: Vec<String> = cols
            .iter()
            .map(|&i| {
                let mut s = format!("{}=", q(&d.columns[i]));
                write_literal(&mut s, row.b[i].as_deref(), cls[i]);
                s
            })
            .collect();
        w.emit(&format!("UPDATE {} SET {} WHERE {}", q(name), sets.join(", "), where_key(d, &cls, &row.a)));
    }
}

fn write_inserts(w: &mut W, name: &str, tb: &Table, d: &TableDataDiff, opts: &ScriptOptions) {
    if d.only_in_b.is_empty() || w.stopped() {
        return;
    }
    let cls = classes(tb, d);
    w.note(&format!("{}: tambah {} baris", q(name), d.only_in_b.len()));
    let cols: Vec<String> = d.columns.iter().map(|c| q(c)).collect();
    let head = format!("INSERT INTO {} ({}) VALUES\n", q(name), cols.join(","));
    let mut s = String::new();
    let mut rows = 0;
    for row in &d.only_in_b {
        if rows == 0 {
            s.push_str(&head);
        } else {
            s.push_str(",\n");
        }
        s.push('(');
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            write_literal(&mut s, cell.as_deref(), cls[i]);
        }
        s.push(')');
        rows += 1;
        if rows >= opts.batch_rows.max(1) || s.len() >= opts.max_statement_bytes {
            w.emit(&s);
            s.clear();
            rows = 0;
            if w.stopped() {
                return;
            }
        }
    }
    if rows > 0 {
        w.emit(&s);
    }
}
