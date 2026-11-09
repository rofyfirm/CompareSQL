//! Normalisasi model supaya noise dump dan beda tampilan MySQL/MariaDB
//! tidak muncul sebagai perbedaan.
//!
//! Dua tingkat:
//! 1. `canonicalize_*`: selalu aman secara semantik, mengubah model
//!    (utf8 -> utf8mb3, `current_timestamp()` -> CURRENT_TIMESTAMP, ...).
//! 2. `CompareOptions`: hal yang diabaikan hanya saat membandingkan
//!    (display width, komentar, charset, engine, urutan kolom).

use serde::{Deserialize, Serialize};

use crate::lexer::canonical_expr_str;
use crate::model::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CompareOptions {
    pub ignore_int_display_width: bool,
    pub ignore_comments: bool,
    pub ignore_charset_collation: bool,
    pub ignore_engine: bool,
    pub ignore_row_format: bool,
    pub ignore_column_order: bool,
    pub ignore_table_name_case: bool,
    pub ignore_auto_increment: bool,
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions {
            ignore_int_display_width: true,
            ignore_comments: false,
            ignore_charset_collation: false,
            ignore_engine: false,
            ignore_row_format: true,
            ignore_column_order: false,
            ignore_table_name_case: true,
            ignore_auto_increment: true,
        }
    }
}

pub fn canonicalize_schema(s: &mut Schema) {
    for t in &mut s.tables {
        canonicalize_table(t);
    }
}

pub fn canonicalize_table(t: &mut Table) {
    let o = &mut t.options;
    o.charset = o.charset.take().map(|c| canon_charset(&c));
    o.collation = o.collation.take().map(|c| canon_collation(&c));
    if o.charset.is_none() {
        o.charset = o.collation.as_deref().map(charset_of_collation);
    }
    o.engine = o.engine.take().map(|e| canon_engine(&e));
    if let Some(c) = o.comment.as_ref() {
        if c.is_empty() {
            o.comment = None;
        }
    }
    let table_cs = o.charset.clone();
    let table_coll = o.collation.clone();
    let engine = o.engine.clone().unwrap_or_default();

    for c in &mut t.columns {
        canonicalize_column(c, table_cs.as_deref(), table_coll.as_deref());
    }
    // Kolom PK selalu NOT NULL.
    if let Some(pk) = &t.primary_key {
        for part in &pk.parts {
            if let Some(name) = &part.column {
                if let Some(c) = t.columns.iter_mut().find(|c| c.name.eq_ignore_ascii_case(name)) {
                    c.nullable = false;
                    if c.default == Some(DefaultValue::Null) {
                        c.default = None;
                    }
                }
            }
        }
    }
    let hash_default = engine.eq_ignore_ascii_case("MEMORY") || engine.eq_ignore_ascii_case("HEAP");
    for idx in t.primary_key.iter_mut().chain(t.indexes.iter_mut()) {
        if idx.using.as_deref() == Some("BTREE") && !hash_default {
            idx.using = None;
        }
        if idx.using.as_deref() == Some("HASH") && hash_default {
            idx.using = None;
        }
        if idx.comment.as_deref() == Some("") {
            idx.comment = None;
        }
    }
    for fk in &mut t.foreign_keys {
        for a in [&mut fk.on_delete, &mut fk.on_update] {
            if matches!(a.as_deref(), Some("RESTRICT") | Some("NO ACTION")) {
                *a = None;
            }
        }
    }
    for ch in &mut t.checks {
        ch.expr = canonical_expr_str(&ch.expr);
    }
}

fn canonicalize_column(c: &mut Column, table_cs: Option<&str>, table_coll: Option<&str>) {
    c.data_type = canon_type(&c.data_type);
    c.charset = c.charset.take().map(|x| canon_charset(&x));
    c.collation = c.collation.take().map(|x| canon_collation(&x));
    if c.charset.is_none() {
        if let Some(coll) = &c.collation {
            c.charset = Some(charset_of_collation(coll));
        }
    }

    // MariaDB: JSON = longtext utf8mb4_bin + CHECK (json_valid(col)).
    if c.data_type == "longtext" {
        if let Some(chk) = &c.check {
            let want = format!("json_valid(`{}`)", c.name);
            if chk.eq_ignore_ascii_case(&want) {
                c.data_type = "json".into();
                c.check = None;
                c.charset = None;
                c.collation = None;
            }
        }
    }

    // Charset/collation sama dengan tabel -> tidak perlu disebut.
    if c.charset.is_some() && c.charset.as_deref() == table_cs {
        let coll_default = match (&c.collation, table_coll) {
            (None, _) => true,
            (Some(cc), Some(tc)) => cc == tc,
            (Some(cc), None) => is_default_collation(cc),
        };
        if coll_default {
            c.charset = None;
            c.collation = None;
        }
    }
    let class = type_class(&c.data_type);
    if !matches!(class, TypeClass::Text) {
        // Hanya tipe teks yang punya charset.
        c.charset = None;
        c.collation = None;
    }

    if c.nullable && c.default == Some(DefaultValue::Null) {
        c.default = None;
    }
    if let Some(d) = c.default.take() {
        c.default = Some(canon_default(d, class));
    }
    if let Some(gen) = &mut c.generated {
        gen.expr = canonical_expr_str(&gen.expr);
    }
    if let Some(chk) = &c.check {
        c.check = Some(canonical_expr_str(chk));
    }
    if c.comment.as_deref() == Some("") {
        c.comment = None;
    }
}

fn canon_default(d: DefaultValue, class: TypeClass) -> DefaultValue {
    match d {
        DefaultValue::Str(s) if class.is_numeric() && class != TypeClass::Bit => {
            let txt = String::from_utf8_lossy(&s).trim().to_string();
            if looks_numeric(txt.as_bytes()) {
                DefaultValue::Num(txt)
            } else {
                DefaultValue::Str(s)
            }
        }
        DefaultValue::Num(n) if !class.is_numeric() && class != TypeClass::Other => {
            // MariaDB menulis DEFAULT 1 untuk varchar? jadikan string.
            DefaultValue::Str(n.into_bytes())
        }
        DefaultValue::Expr(e) => {
            let e = canonical_expr_str(&e);
            match e.as_str() {
                "current_timestamp()" | "now()" | "localtime()" | "localtimestamp()" => {
                    DefaultValue::CurrentTimestamp(None)
                }
                _ => DefaultValue::Expr(e),
            }
        }
        other => other,
    }
}

pub fn looks_numeric(s: &[u8]) -> bool {
    let mut i = 0;
    if s.first().is_some_and(|&c| c == b'-' || c == b'+') {
        i = 1;
    }
    let mut digits = 0;
    let mut dot = false;
    let mut exp = false;
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_digit() {
            digits += 1;
        } else if c == b'.' && !dot && !exp {
            dot = true;
        } else if (c == b'e' || c == b'E') && digits > 0 && !exp {
            exp = true;
            if s.get(i + 1).is_some_and(|&c| c == b'-' || c == b'+') {
                i += 1;
            }
        } else {
            return false;
        }
        i += 1;
    }
    digits > 0
}

pub fn canon_charset(c: &str) -> String {
    let c = c.to_ascii_lowercase();
    if c == "utf8" {
        "utf8mb3".into()
    } else {
        c
    }
}

pub fn canon_collation(c: &str) -> String {
    let c = c.to_ascii_lowercase();
    if let Some(rest) = c.strip_prefix("utf8_") {
        format!("utf8mb3_{rest}")
    } else {
        c
    }
}

pub fn charset_of_collation(coll: &str) -> String {
    let cs = coll.split('_').next().unwrap_or(coll);
    canon_charset(cs)
}

/// Collation default untuk charset umum di MySQL 5.7/8 dan MariaDB 10/11.
pub fn is_default_collation(coll: &str) -> bool {
    matches!(
        coll,
        "utf8mb4_general_ci"
            | "utf8mb4_0900_ai_ci"
            | "utf8mb4_uca1400_ai_ci"
            | "utf8mb3_general_ci"
            | "utf8mb3_uca1400_ai_ci"
            | "latin1_swedish_ci"
            | "ascii_general_ci"
            | "binary"
    )
}

fn canon_engine(e: &str) -> String {
    match e.to_ascii_lowercase().as_str() {
        "innodb" => "InnoDB".into(),
        "myisam" => "MyISAM".into(),
        "memory" | "heap" => "MEMORY".into(),
        "aria" => "Aria".into(),
        "csv" => "CSV".into(),
        "archive" => "ARCHIVE".into(),
        "blackhole" => "BLACKHOLE".into(),
        "merge" | "mrg_myisam" => "MRG_MyISAM".into(),
        _ => e.to_string(),
    }
}

/// Alias tipe -> nama kanonik. Display width dipertahankan.
pub fn canon_type(t: &str) -> String {
    // Huruf kecil hanya di luar kurung: nilai enum/set peka huruf besar.
    let t = t.trim();
    let (base, params, attrs) = match t.find('(') {
        Some(p) => {
            let close = t.rfind(')').unwrap_or(t.len() - 1);
            (&t[..p], &t[p..=close], &t[close + 1..])
        }
        None => match t.find(' ') {
            Some(p) => (&t[..p], "", &t[p..]),
            None => (t, "", ""),
        },
    };
    let base = base.to_ascii_lowercase();
    let rest = format!("{params}{}", attrs.to_ascii_lowercase());
    let base = base.as_str();
    let rest = rest.as_str();
    let base2 = match base {
        "integer" | "int4" => "int",
        "int1" => "tinyint",
        "int2" => "smallint",
        "int3" | "middleint" => "mediumint",
        "int8" => "bigint",
        "bool" | "boolean" => return "tinyint(1)".into(),
        "dec" | "numeric" | "fixed" => "decimal",
        "real" | "float8" => "double",
        "float4" => "float",
        "character" => "char",
        other => other,
    };
    let mut s = format!("{base2}{rest}");
    if s == "decimal" {
        s = "decimal(10,0)".into();
    }
    if s.starts_with("decimal(") && !s.contains(',') {
        // decimal(10) -> decimal(10,0)
        s = s.replacen(')', ",0)", 1);
    }
    s
}

/// Buang display width integer: `int(11) unsigned` -> `int unsigned`.
pub fn strip_int_width(t: &str) -> String {
    let b = base_type(t);
    if matches!(type_class(t), TypeClass::Integer) && t[b.len()..].starts_with('(') {
        if let Some(end) = t.find(')') {
            return format!("{}{}", b, &t[end + 1..]);
        }
    }
    t.to_string()
}

/// Bandingkan collation; None di satu sisi dianggap sama bila sisi lain
/// adalah collation default untuk charset.
pub fn collation_eq(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x == y,
        (None, None) => true,
        (Some(x), None) | (None, Some(x)) => is_default_collation(x),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddl::{classify, Stmt};

    fn table(sql: &str) -> Table {
        match classify(sql.as_bytes()).unwrap() {
            Stmt::CreateTable(mut t) => {
                canonicalize_table(&mut t);
                t
            }
            _ => panic!(),
        }
    }

    #[test]
    fn mysql_and_mariadb_render_same_model() {
        let my = table(
            "CREATE TABLE `t` (`id` int NOT NULL AUTO_INCREMENT, `n` int NOT NULL DEFAULT '0', \
             `d` datetime DEFAULT CURRENT_TIMESTAMP, `j` json DEFAULT NULL, `x` text, \
             `s` varchar(10) CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci DEFAULT NULL, \
             PRIMARY KEY (`id`)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        );
        let ma = table(
            "CREATE TABLE `t` (`id` int(11) NOT NULL AUTO_INCREMENT, `n` int(11) NOT NULL DEFAULT 0, \
             `d` datetime DEFAULT current_timestamp(), \
             `j` longtext CHARACTER SET utf8mb4 COLLATE utf8mb4_bin DEFAULT NULL CHECK (json_valid(`j`)), \
             `x` text DEFAULT NULL, `s` varchar(10) DEFAULT NULL, \
             PRIMARY KEY (`id`) USING BTREE) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci",
        );
        for (a, b) in my.columns.iter().zip(&ma.columns) {
            assert_eq!(strip_int_width(&a.data_type), strip_int_width(&b.data_type), "{}", a.name);
            assert_eq!(a.default, b.default, "{}", a.name);
            assert_eq!(a.check, b.check, "{}", a.name);
            assert_eq!(a.charset, b.charset, "{}", a.name);
            assert_eq!(a.collation, b.collation, "{}", a.name);
        }
        assert_eq!(my.primary_key, ma.primary_key);
    }

    #[test]
    fn numeric_check() {
        assert!(looks_numeric(b"-1.5e-3"));
        assert!(looks_numeric(b"0.00"));
        assert!(!looks_numeric(b"abc"));
        assert!(!looks_numeric(b""));
        assert_eq!(strip_int_width("int(11) unsigned"), "int unsigned");
        assert_eq!(strip_int_width("varchar(11)"), "varchar(11)");
        assert_eq!(canon_type("numeric(10)"), "decimal(10,0)");
        assert_eq!(canon_type("ENUM('L','P')"), "enum('L','P')");
        assert_eq!(canon_type("INT(11) UNSIGNED"), "int(11) unsigned");
    }
}
