//! Render model kembali ke SQL kanonik (gaya SHOW CREATE TABLE).

use crate::lexer::{hex_encode, quote_ident, quote_str};
use crate::model::*;

pub fn q(name: &str) -> String {
    quote_ident(name)
}

pub fn render_default(d: &DefaultValue) -> String {
    match d {
        DefaultValue::Null => "NULL".into(),
        DefaultValue::Str(s) => quote_str(s),
        DefaultValue::Num(n) => n.clone(),
        DefaultValue::Bits(b) => format!("b'{b}'"),
        DefaultValue::Hex(h) => format!("0x{}", hex_encode(h)),
        DefaultValue::CurrentTimestamp(None) => "CURRENT_TIMESTAMP".into(),
        DefaultValue::CurrentTimestamp(Some(p)) => format!("CURRENT_TIMESTAMP({p})"),
        DefaultValue::Expr(e) => format!("({e})"),
    }
}

pub fn render_column(c: &Column) -> String {
    let mut s = format!("{} {}", q(&c.name), c.data_type);
    if let Some(cs) = &c.charset {
        s.push_str(&format!(" CHARACTER SET {cs}"));
    }
    if let Some(co) = &c.collation {
        s.push_str(&format!(" COLLATE {co}"));
    }
    if let Some(g) = &c.generated {
        s.push_str(&format!(
            " GENERATED ALWAYS AS ({}) {}",
            g.expr,
            if g.stored { "STORED" } else { "VIRTUAL" }
        ));
        if !c.nullable {
            s.push_str(" NOT NULL");
        }
    } else {
        s.push_str(if c.nullable { " NULL" } else { " NOT NULL" });
        if let Some(d) = &c.default {
            s.push_str(" DEFAULT ");
            s.push_str(&render_default(d));
        }
        if let Some(u) = &c.on_update {
            s.push_str(" ON UPDATE ");
            s.push_str(u);
        }
        if c.auto_increment {
            s.push_str(" AUTO_INCREMENT");
        }
    }
    if c.invisible {
        s.push_str(" INVISIBLE");
    }
    for e in &c.extra {
        s.push(' ');
        s.push_str(e);
    }
    if let Some(cm) = &c.comment {
        s.push_str(" COMMENT ");
        s.push_str(&quote_str(cm.as_bytes()));
    }
    if let Some(chk) = &c.check {
        s.push_str(&format!(" CHECK ({chk})"));
    }
    s
}

fn render_parts(parts: &[IndexPart]) -> String {
    let v: Vec<String> = parts
        .iter()
        .map(|p| {
            let mut s = match (&p.column, &p.expr) {
                (Some(c), _) => q(c),
                (None, Some(e)) => format!("({e})"),
                _ => String::new(),
            };
            if let Some(n) = p.prefix {
                s.push_str(&format!("({n})"));
            }
            if p.desc {
                s.push_str(" DESC");
            }
            s
        })
        .collect();
    format!("({})", v.join(","))
}

/// Definisi index untuk CREATE TABLE (tanpa ADD).
pub fn render_index(i: &Index) -> String {
    let head = match i.kind {
        IndexKind::Primary => "PRIMARY KEY".to_string(),
        IndexKind::Unique => format!("UNIQUE KEY {}", q(&i.name)),
        IndexKind::Index => format!("KEY {}", q(&i.name)),
        IndexKind::Fulltext => format!("FULLTEXT KEY {}", q(&i.name)),
        IndexKind::Spatial => format!("SPATIAL KEY {}", q(&i.name)),
    };
    let mut s = format!("{head} {}", render_parts(&i.parts));
    if let Some(u) = &i.using {
        s.push_str(&format!(" USING {u}"));
    }
    for e in &i.extra {
        s.push(' ');
        s.push_str(e);
    }
    if let Some(c) = &i.comment {
        s.push_str(&format!(" COMMENT {}", quote_str(c.as_bytes())));
    }
    if i.invisible {
        s.push_str(" /*!80000 INVISIBLE */");
    }
    s
}

pub fn render_fk(fk: &ForeignKey) -> String {
    let cols: Vec<String> = fk.columns.iter().map(|c| q(c)).collect();
    let refs: Vec<String> = fk.ref_columns.iter().map(|c| q(c)).collect();
    let target = match &fk.ref_schema {
        Some(s) => format!("{}.{}", q(s), q(&fk.ref_table)),
        None => q(&fk.ref_table),
    };
    let mut s = format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        q(&fk.name),
        cols.join(", "),
        target,
        refs.join(", ")
    );
    if let Some(a) = &fk.on_delete {
        s.push_str(&format!(" ON DELETE {a}"));
    }
    if let Some(a) = &fk.on_update {
        s.push_str(&format!(" ON UPDATE {a}"));
    }
    s
}

pub fn render_check(c: &Check) -> String {
    let mut s = match &c.name {
        Some(n) => format!("CONSTRAINT {} CHECK ({})", q(n), c.expr),
        None => format!("CHECK ({})", c.expr),
    };
    if !c.enforced {
        s.push_str(" /*!80016 NOT ENFORCED */");
    }
    s
}

/// Opsi tabel. `with_auto_inc` hanya untuk tampilan.
pub fn render_table_options(o: &TableOptions, with_auto_inc: bool) -> String {
    let mut v = vec![];
    if let Some(e) = &o.engine {
        v.push(format!("ENGINE={e}"));
    }
    if with_auto_inc {
        if let Some(n) = o.auto_increment {
            v.push(format!("AUTO_INCREMENT={n}"));
        }
    }
    if let Some(c) = &o.charset {
        v.push(format!("DEFAULT CHARSET={c}"));
    }
    if let Some(c) = &o.collation {
        v.push(format!("COLLATE={c}"));
    }
    if let Some(r) = &o.row_format {
        v.push(format!("ROW_FORMAT={r}"));
    }
    for (k, val) in &o.other {
        if k == "PERIOD" {
            continue;
        }
        if val.is_empty() {
            v.push(k.clone());
        } else {
            v.push(format!("{k}={val}"));
        }
    }
    if let Some(c) = &o.comment {
        v.push(format!("COMMENT={}", quote_str(c.as_bytes())));
    }
    v.join(" ")
}

/// CREATE TABLE kanonik. `with_fks=false` untuk script (FK ditambah di akhir).
pub fn render_create_table(t: &Table, with_fks: bool, with_auto_inc: bool) -> String {
    let mut lines: Vec<String> = t.columns.iter().map(render_column).collect();
    if let Some(pk) = &t.primary_key {
        lines.push(render_index(pk));
    }
    for i in &t.indexes {
        lines.push(render_index(i));
    }
    if with_fks {
        for fk in &t.foreign_keys {
            lines.push(render_fk(fk));
        }
    }
    for c in &t.checks {
        lines.push(render_check(c));
    }
    if let Some(p) = t.options.other.get("PERIOD") {
        lines.push(p.clone());
    }
    let mut s = format!("CREATE TABLE {} (\n  {}\n)", q(&t.name), lines.join(",\n  "));
    let opts = render_table_options(&t.options, with_auto_inc);
    if !opts.is_empty() {
        s.push(' ');
        s.push_str(&opts);
    }
    if let Some(p) = &t.options.partition {
        s.push('\n');
        s.push_str(p);
    }
    s
}
