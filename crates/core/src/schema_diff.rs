//! Schema diff: cocokkan objek berdasarkan nama, laporkan perubahan A -> B.
//! "added" = ada di B, belum ada di A. "removed" = ada di A, hilang di B.

use serde::Serialize;

use crate::model::*;
use crate::normalize::{collation_eq, strip_int_width, CompareOptions};
use crate::render::{render_default, render_index};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Same,
    Modified,
    OnlyInA,
    OnlyInB,
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// column | primary_key | index | foreign_key | check | option
    pub category: &'static str,
    /// added | removed | modified | moved
    pub action: &'static str,
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableSchemaDiff {
    pub name: String,
    pub name_a: Option<String>,
    pub name_b: Option<String>,
    pub status: Status,
    pub changes: Vec<Change>,
}

/// Pasangkan tabel A dan B berdasarkan nama, urut nama.
pub fn pair_tables<'a>(
    a: &'a Schema,
    b: &'a Schema,
    opts: &CompareOptions,
) -> Vec<(Option<&'a Table>, Option<&'a Table>)> {
    let eq = |x: &str, y: &str| {
        if opts.ignore_table_name_case {
            x.eq_ignore_ascii_case(y)
        } else {
            x == y
        }
    };
    let mut out: Vec<(Option<&Table>, Option<&Table>)> = vec![];
    let mut used_b = vec![false; b.tables.len()];
    for ta in &a.tables {
        let pos = b.tables.iter().enumerate().position(|(i, tb)| !used_b[i] && eq(&ta.name, &tb.name));
        match pos {
            Some(i) => {
                used_b[i] = true;
                out.push((Some(ta), Some(&b.tables[i])));
            }
            None => out.push((Some(ta), None)),
        }
    }
    for (i, tb) in b.tables.iter().enumerate() {
        if !used_b[i] {
            out.push((None, Some(tb)));
        }
    }
    out.sort_by_key(|(x, y)| x.or(*y).map(|t| t.name.to_ascii_lowercase()).unwrap_or_default());
    out
}

pub fn diff_schemas(a: &Schema, b: &Schema, opts: &CompareOptions) -> Vec<TableSchemaDiff> {
    pair_tables(a, b, opts)
        .into_iter()
        .map(|(ta, tb)| {
            let name = ta.or(tb).map(|t| t.name.clone()).unwrap_or_default();
            let (status, changes) = match (ta, tb) {
                (Some(x), Some(y)) => {
                    let ch = diff_table(x, y, opts);
                    (if ch.is_empty() { Status::Same } else { Status::Modified }, ch)
                }
                (Some(_), None) => (Status::OnlyInA, vec![]),
                _ => (Status::OnlyInB, vec![]),
            };
            TableSchemaDiff {
                name,
                name_a: ta.map(|t| t.name.clone()),
                name_b: tb.map(|t| t.name.clone()),
                status,
                changes,
            }
        })
        .collect()
}

fn type_for_compare(t: &str, opts: &CompareOptions) -> String {
    let mut s = t.to_string();
    if opts.ignore_int_display_width {
        s = strip_int_width(&s);
        if s == "year(4)" {
            s = "year".into();
        }
    }
    s
}

fn opt_txt<T: AsRef<str>>(v: &Option<T>) -> String {
    v.as_ref().map(|s| s.as_ref().to_string()).unwrap_or_else(|| "-".into())
}

fn default_txt(d: &Option<DefaultValue>) -> String {
    d.as_ref().map(render_default).unwrap_or_else(|| "(tidak ada)".into())
}

/// Daftar beda dua definisi kolom; kosong = sama.
pub fn column_diffs(a: &Column, b: &Column, opts: &CompareOptions) -> Vec<String> {
    let mut v = vec![];
    let (ta, tb) = (type_for_compare(&a.data_type, opts), type_for_compare(&b.data_type, opts));
    if ta != tb {
        v.push(format!("tipe {} → {}", a.data_type, b.data_type));
    }
    if a.nullable != b.nullable {
        let f = |n: bool| if n { "NULL" } else { "NOT NULL" };
        v.push(format!("{} → {}", f(a.nullable), f(b.nullable)));
    }
    if a.default != b.default {
        v.push(format!("default {} → {}", default_txt(&a.default), default_txt(&b.default)));
    }
    if a.auto_increment != b.auto_increment {
        v.push(format!("auto_increment {} → {}", a.auto_increment, b.auto_increment));
    }
    if a.on_update != b.on_update {
        v.push(format!("on update {} → {}", opt_txt(&a.on_update), opt_txt(&b.on_update)));
    }
    if a.generated != b.generated {
        let g = |x: &Option<Generated>| {
            x.as_ref()
                .map(|g| format!("{} {}", g.expr, if g.stored { "STORED" } else { "VIRTUAL" }))
                .unwrap_or_else(|| "-".into())
        };
        v.push(format!("generated {} → {}", g(&a.generated), g(&b.generated)));
    }
    if a.invisible != b.invisible {
        v.push(format!("invisible {} → {}", a.invisible, b.invisible));
    }
    if a.check != b.check {
        v.push(format!("check {} → {}", opt_txt(&a.check), opt_txt(&b.check)));
    }
    if a.extra != b.extra {
        v.push(format!("atribut {:?} → {:?}", a.extra, b.extra));
    }
    if !opts.ignore_charset_collation {
        if a.charset != b.charset {
            v.push(format!("charset {} → {}", opt_txt(&a.charset), opt_txt(&b.charset)));
        } else if !collation_eq(a.collation.as_deref(), b.collation.as_deref()) {
            v.push(format!("collation {} → {}", opt_txt(&a.collation), opt_txt(&b.collation)));
        }
    }
    if !opts.ignore_comments && a.comment != b.comment {
        v.push(format!("komentar {} → {}", opt_txt(&a.comment), opt_txt(&b.comment)));
    }
    v
}

fn part_eq(a: &IndexPart, b: &IndexPart) -> bool {
    let col = match (&a.column, &b.column) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
        (None, None) => true,
        _ => false,
    };
    col && a.expr == b.expr && a.prefix == b.prefix && a.desc == b.desc
}

pub fn index_eq(a: &Index, b: &Index, opts: &CompareOptions) -> bool {
    a.kind == b.kind
        && a.parts.len() == b.parts.len()
        && a.parts.iter().zip(&b.parts).all(|(x, y)| part_eq(x, y))
        && a.using == b.using
        && a.invisible == b.invisible
        && a.extra == b.extra
        && (opts.ignore_comments || a.comment == b.comment)
}

fn names_eq(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// Definisi FK sama (nama tidak ikut dibandingkan).
pub fn fk_def_eq(a: &ForeignKey, b: &ForeignKey, opts: &CompareOptions) -> bool {
    let table_eq = if opts.ignore_table_name_case {
        a.ref_table.eq_ignore_ascii_case(&b.ref_table)
    } else {
        a.ref_table == b.ref_table
    };
    table_eq
        && names_eq(&a.columns, &b.columns)
        && names_eq(&a.ref_columns, &b.ref_columns)
        && a.on_delete == b.on_delete
        && a.on_update == b.on_update
}

/// Pasangan FK: dicocokkan per nama, lalu per definisi.
/// Hasil: (pasangan, hanya_A, hanya_B).
pub fn match_fks<'a>(
    a: &'a [ForeignKey],
    b: &'a [ForeignKey],
    opts: &CompareOptions,
) -> (Vec<(&'a ForeignKey, &'a ForeignKey)>, Vec<&'a ForeignKey>, Vec<&'a ForeignKey>) {
    let mut used_b = vec![false; b.len()];
    let mut pairs = vec![];
    let mut only_a = vec![];
    for fa in a {
        let by_name = b.iter().enumerate().position(|(i, fb)| !used_b[i] && fb.name.eq_ignore_ascii_case(&fa.name));
        let pos = by_name.or_else(|| b.iter().enumerate().position(|(i, fb)| !used_b[i] && fk_def_eq(fa, fb, opts)));
        match pos {
            Some(i) => {
                used_b[i] = true;
                pairs.push((fa, &b[i]));
            }
            None => only_a.push(fa),
        }
    }
    let only_b = b.iter().enumerate().filter(|(i, _)| !used_b[*i]).map(|(_, f)| f).collect();
    (pairs, only_a, only_b)
}

/// CHECK dicocokkan per ekspresi (nama otomatis beda antar server).
pub fn match_checks<'a>(a: &'a [Check], b: &'a [Check]) -> (Vec<&'a Check>, Vec<&'a Check>) {
    let mut used_b = vec![false; b.len()];
    let mut only_a = vec![];
    for ca in a {
        match b.iter().enumerate().position(|(i, cb)| !used_b[i] && cb.expr == ca.expr && cb.enforced == ca.enforced) {
            Some(i) => used_b[i] = true,
            None => only_a.push(ca),
        }
    }
    let only_b = b.iter().enumerate().filter(|(i, _)| !used_b[*i]).map(|(_, c)| c).collect();
    (only_a, only_b)
}

/// Kolom bersama yang posisinya berubah (tidak ada di LCS urutan A vs B).
pub fn moved_columns(a: &Table, b: &Table) -> Vec<String> {
    let common_a: Vec<String> = a
        .columns
        .iter()
        .filter(|c| b.column(&c.name).is_some())
        .map(|c| c.name.to_ascii_lowercase())
        .collect();
    let common_b: Vec<String> = b
        .columns
        .iter()
        .filter(|c| a.column(&c.name).is_some())
        .map(|c| c.name.to_ascii_lowercase())
        .collect();
    let (n, m) = (common_a.len(), common_b.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if common_a[i] == common_b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut in_lcs = std::collections::HashSet::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if common_a[i] == common_b[j] {
            in_lcs.insert(common_b[j].clone());
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    b.columns
        .iter()
        .filter(|c| a.column(&c.name).is_some() && !in_lcs.contains(&c.name.to_ascii_lowercase()))
        .map(|c| c.name.clone())
        .collect()
}

const NOISE_OPTIONS: &[&str] = &[
    "PAGE_CHECKSUM",
    "TRANSACTIONAL",
    "STATS_PERSISTENT",
    "STATS_AUTO_RECALC",
    "STATS_SAMPLE_PAGES",
    "AVG_ROW_LENGTH",
    "CHECKSUM",
    "DELAY_KEY_WRITE",
    "PACK_KEYS",
    "PERIOD",
];

pub fn option_diffs(a: &TableOptions, b: &TableOptions, opts: &CompareOptions) -> Vec<Change> {
    let mut v = vec![];
    let mut push = |name: &str, x: String, y: String| {
        v.push(Change { category: "option", action: "modified", name: name.into(), detail: format!("{x} → {y}") });
    };
    if !opts.ignore_engine {
        let e = |x: &Option<String>| x.as_deref().map(|s| s.to_ascii_lowercase());
        if e(&a.engine) != e(&b.engine) {
            push("ENGINE", opt_txt(&a.engine), opt_txt(&b.engine));
        }
    }
    if !opts.ignore_charset_collation {
        if a.charset != b.charset {
            push("CHARSET", opt_txt(&a.charset), opt_txt(&b.charset));
        } else if !collation_eq(a.collation.as_deref(), b.collation.as_deref()) {
            push("COLLATE", opt_txt(&a.collation), opt_txt(&b.collation));
        }
    }
    if !opts.ignore_comments && a.comment != b.comment {
        push("COMMENT", opt_txt(&a.comment), opt_txt(&b.comment));
    }
    if !opts.ignore_row_format && a.row_format != b.row_format {
        push("ROW_FORMAT", opt_txt(&a.row_format), opt_txt(&b.row_format));
    }
    if !opts.ignore_auto_increment && a.auto_increment != b.auto_increment {
        let f = |x: Option<u64>| x.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
        push("AUTO_INCREMENT", f(a.auto_increment), f(b.auto_increment));
    }
    if a.partition != b.partition {
        push("PARTITION", opt_txt(&a.partition), opt_txt(&b.partition));
    }
    let keys: std::collections::BTreeSet<&String> = a.other.keys().chain(b.other.keys()).collect();
    for k in keys {
        if NOISE_OPTIONS.contains(&k.as_str()) {
            continue;
        }
        if a.other.get(k) != b.other.get(k) {
            push(k, opt_txt(&a.other.get(k)), opt_txt(&b.other.get(k)));
        }
    }
    v
}

pub fn diff_table(a: &Table, b: &Table, opts: &CompareOptions) -> Vec<Change> {
    let mut out = vec![];
    // Kolom
    for ca in &a.columns {
        if b.column(&ca.name).is_none() {
            out.push(Change { category: "column", action: "removed", name: ca.name.clone(), detail: ca.data_type.clone() });
        }
    }
    for cb in &b.columns {
        match a.column(&cb.name) {
            None => out.push(Change { category: "column", action: "added", name: cb.name.clone(), detail: cb.data_type.clone() }),
            Some(ca) => {
                let d = column_diffs(ca, cb, opts);
                if !d.is_empty() {
                    out.push(Change { category: "column", action: "modified", name: cb.name.clone(), detail: d.join("; ") });
                }
            }
        }
    }
    if !opts.ignore_column_order {
        for name in moved_columns(a, b) {
            out.push(Change { category: "column", action: "moved", name, detail: "urutan kolom berubah".into() });
        }
    }
    // Primary key
    match (&a.primary_key, &b.primary_key) {
        (Some(x), Some(y)) if !index_eq(x, y, opts) => out.push(Change {
            category: "primary_key",
            action: "modified",
            name: "PRIMARY".into(),
            detail: format!("{} → {}", render_index(x), render_index(y)),
        }),
        (Some(x), None) => out.push(Change { category: "primary_key", action: "removed", name: "PRIMARY".into(), detail: render_index(x) }),
        (None, Some(y)) => out.push(Change { category: "primary_key", action: "added", name: "PRIMARY".into(), detail: render_index(y) }),
        _ => {}
    }
    // Index
    for ia in &a.indexes {
        match b.indexes.iter().find(|ib| ib.name.eq_ignore_ascii_case(&ia.name)) {
            None => out.push(Change { category: "index", action: "removed", name: ia.name.clone(), detail: render_index(ia) }),
            Some(ib) if !index_eq(ia, ib, opts) => out.push(Change {
                category: "index",
                action: "modified",
                name: ia.name.clone(),
                detail: format!("{} → {}", render_index(ia), render_index(ib)),
            }),
            _ => {}
        }
    }
    for ib in &b.indexes {
        if !a.indexes.iter().any(|ia| ia.name.eq_ignore_ascii_case(&ib.name)) {
            out.push(Change { category: "index", action: "added", name: ib.name.clone(), detail: render_index(ib) });
        }
    }
    // Foreign key
    let (pairs, only_a, only_b) = match_fks(&a.foreign_keys, &b.foreign_keys, opts);
    for (fa, fb) in pairs {
        if !fk_def_eq(fa, fb, opts) {
            out.push(Change {
                category: "foreign_key",
                action: "modified",
                name: fb.name.clone(),
                detail: format!("{} → {}", crate::render::render_fk(fa), crate::render::render_fk(fb)),
            });
        }
    }
    for f in only_a {
        out.push(Change { category: "foreign_key", action: "removed", name: f.name.clone(), detail: crate::render::render_fk(f) });
    }
    for f in only_b {
        out.push(Change { category: "foreign_key", action: "added", name: f.name.clone(), detail: crate::render::render_fk(f) });
    }
    // Check
    let (ca, cb) = match_checks(&a.checks, &b.checks);
    for c in ca {
        out.push(Change { category: "check", action: "removed", name: c.name.clone().unwrap_or_default(), detail: c.expr.clone() });
    }
    for c in cb {
        out.push(Change { category: "check", action: "added", name: c.name.clone().unwrap_or_default(), detail: c.expr.clone() });
    }
    out.extend(option_diffs(&a.options, &b.options, opts));
    out
}
