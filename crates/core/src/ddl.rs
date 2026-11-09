//! Parser DDL: klasifikasi statement dan `CREATE TABLE` -> `model::Table`.
//!
//! Parser ini khusus format `SHOW CREATE TABLE` / mysqldump (MySQL 5.7, 8.x,
//! MariaDB 10.x/11.x), tetapi toleran pada DDL tulisan tangan.

use crate::lexer::{canonical_expr, quote_str, render_tokens, tok_text, Tok, Tokenizer};
use crate::model::*;
use crate::{Error, Result};

#[derive(Debug)]
pub enum Stmt {
    CreateTable(Table),
    DropTables(Vec<String>),
    Insert(String),
    Skipped(SkippedObject),
    /// Statement yang tidak didukung tetapi relevan (dilaporkan sebagai warning).
    Unsupported(String),
    Ignore,
}

/// Klasifikasi cepat dari awal statement; CREATE TABLE di-parse penuh.
pub fn classify(stmt: &[u8]) -> Result<Stmt> {
    let head = &stmt[..stmt.len().min(4096)];
    let mut tz = Tokenizer::new(head);
    let mut toks: Vec<Tok> = Vec::with_capacity(48);
    while toks.len() < 48 {
        match tz.next_tok() {
            Some(t) => toks.push(t),
            None => break,
        }
    }
    let Some(first) = toks.first() else { return Ok(Stmt::Ignore) };
    let w = |i: usize, s: &str| toks.get(i).is_some_and(|t| t.is_word(s));

    if first.is_word("CREATE") {
        const KINDS: [&str; 10] =
            ["TABLE", "VIEW", "TRIGGER", "PROCEDURE", "FUNCTION", "EVENT", "DATABASE", "SCHEMA", "INDEX", "SEQUENCE"];
        let pos = toks
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, t)| KINDS.iter().any(|k| t.is_word(k)));
        let Some((pos, kw)) = pos else { return Ok(Stmt::Ignore) };
        let kind = match kw {
            Tok::Word(s) => s.to_ascii_uppercase(),
            _ => unreachable!(),
        };
        match kind.as_str() {
            "TABLE" => {
                let full: Vec<Tok> = Tokenizer::new(stmt).collect();
                return Ok(match parse_create_table(&full)? {
                    Some(t) => Stmt::CreateTable(t),
                    None => Stmt::Unsupported("CREATE TABLE ... LIKE/SELECT".into()),
                });
            }
            "DATABASE" | "SCHEMA" => return Ok(Stmt::Ignore),
            "INDEX" => return Ok(Stmt::Unsupported("CREATE INDEX terpisah".into())),
            _ => {
                let mut i = pos + 1;
                if w(i, "IF") {
                    i += 3;
                }
                let name = qualified_last(&toks, i).unwrap_or_default();
                return Ok(Stmt::Skipped(SkippedObject { kind: kind.to_ascii_lowercase(), name }));
            }
        }
    }
    if first.is_word("DROP") && w(1, "TABLE") {
        let mut i = 2;
        if w(i, "IF") && w(i + 1, "EXISTS") {
            i += 2;
        }
        let mut names = vec![];
        while i < toks.len() {
            if let Some(n) = qualified_last(&toks, i) {
                names.push(n);
            }
            // lompat ke koma berikutnya
            while i < toks.len() && !toks[i].is_punct(",") {
                i += 1;
            }
            i += 1;
        }
        return Ok(Stmt::DropTables(names));
    }
    if first.is_word("INSERT") || first.is_word("REPLACE") {
        let mut i = 1;
        while toks.get(i).is_some_and(|t| {
            ["LOW_PRIORITY", "DELAYED", "HIGH_PRIORITY", "IGNORE", "INTO"].iter().any(|k| t.is_word(k))
        }) {
            i += 1;
        }
        return match qualified_last(&toks, i) {
            Some(n) => Ok(Stmt::Insert(n)),
            None => Err(Error::parse("nama tabel INSERT tidak terbaca")),
        };
    }
    if first.is_word("ALTER") && w(1, "TABLE") {
        if toks.iter().any(|t| t.is_word("DISABLE") || t.is_word("ENABLE")) && toks.iter().any(|t| t.is_word("KEYS")) {
            return Ok(Stmt::Ignore);
        }
        return Ok(Stmt::Unsupported("ALTER TABLE di dalam dump".into()));
    }
    Ok(Stmt::Ignore)
}

/// Ambil nama dari `a` atau `a`.`b` mulai posisi i; kembalikan bagian terakhir.
fn qualified_last(toks: &[Tok], i: usize) -> Option<String> {
    let mut name = toks.get(i)?.name()?.to_string();
    let mut j = i + 1;
    while toks.get(j).is_some_and(|t| t.is_punct(".")) {
        if let Some(n) = toks.get(j + 1).and_then(|t| t.name()) {
            name = n.to_string();
        }
        j += 2;
    }
    Some(name)
}

// ---------------------------------------------------------------------------

struct P<'a> {
    t: &'a [Tok],
    i: usize,
}

impl<'a> P<'a> {
    fn new(t: &'a [Tok]) -> Self {
        P { t, i: 0 }
    }
    fn peek(&self) -> Option<&'a Tok> {
        self.t.get(self.i)
    }
    fn peek_at(&self, k: usize) -> Option<&'a Tok> {
        self.t.get(self.i + k)
    }
    fn next(&mut self) -> Option<&'a Tok> {
        let t = self.t.get(self.i);
        if t.is_some() {
            self.i += 1;
        }
        t
    }
    fn done(&self) -> bool {
        self.i >= self.t.len()
    }
    fn is_word(&self, w: &str) -> bool {
        self.peek().is_some_and(|t| t.is_word(w))
    }
    fn is_punct(&self, p: &str) -> bool {
        self.peek().is_some_and(|t| t.is_punct(p))
    }
    fn eat_word(&mut self, w: &str) -> bool {
        if self.is_word(w) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn eat_words(&mut self, ws: &[&str]) -> bool {
        for (k, w) in ws.iter().enumerate() {
            if !self.peek_at(k).is_some_and(|t| t.is_word(w)) {
                return false;
            }
        }
        self.i += ws.len();
        true
    }
    fn eat_punct(&mut self, p: &str) -> bool {
        if self.is_punct(p) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expect_punct(&mut self, p: &str) -> Result<()> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            Err(Error::parse(format!("butuh '{p}', dapat {:?}", self.peek())))
        }
    }
    fn name(&mut self) -> Result<String> {
        match self.next() {
            Some(Tok::Word(s)) | Some(Tok::Ident(s)) => Ok(s.clone()),
            Some(Tok::Str(s)) => Ok(String::from_utf8_lossy(s).into_owned()),
            t => Err(Error::parse(format!("butuh nama, dapat {t:?}"))),
        }
    }
    /// `a`.`b` -> (Some(a), b)
    fn qualified(&mut self) -> Result<(Option<String>, String)> {
        let first = self.name()?;
        if self.is_punct(".") && self.peek_at(1).and_then(|t| t.name()).is_some() {
            self.i += 1;
            let second = self.name()?;
            return Ok((Some(first), second));
        }
        Ok((None, first))
    }
    /// Di posisi '(' : ambil token di dalam kurung (seimbang) dan lewati ')'.
    fn group(&mut self) -> Result<&'a [Tok]> {
        self.expect_punct("(")?;
        let start = self.i;
        let mut depth = 1;
        while let Some(t) = self.next() {
            if t.is_punct("(") {
                depth += 1;
            } else if t.is_punct(")") {
                depth -= 1;
                if depth == 0 {
                    return Ok(&self.t[start..self.i - 1]);
                }
            }
        }
        Err(Error::parse("kurung tidak tertutup"))
    }
    /// `= value` opsional setelah nama opsi.
    fn opt_eq(&mut self) {
        self.eat_punct("=");
    }
    fn string(&mut self) -> Result<String> {
        match self.next() {
            Some(Tok::Str(s)) => Ok(String::from_utf8_lossy(s).into_owned()),
            t => Err(Error::parse(format!("butuh string, dapat {t:?}"))),
        }
    }
}

/// Pecah token per koma di kedalaman 0.
fn split_commas(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut out = vec![];
    let mut depth = 0i32;
    let mut start = 0;
    for (i, t) in toks.iter().enumerate() {
        if t.is_punct("(") {
            depth += 1;
        } else if t.is_punct(")") {
            depth -= 1;
        } else if t.is_punct(",") && depth == 0 {
            out.push(&toks[start..i]);
            start = i + 1;
        }
    }
    if start < toks.len() {
        out.push(&toks[start..]);
    }
    out
}

/// Parse `CREATE TABLE`. `Ok(None)` untuk bentuk LIKE / AS SELECT.
pub fn parse_create_table(toks: &[Tok]) -> Result<Option<Table>> {
    let mut p = P::new(toks);
    if !p.eat_word("CREATE") {
        return Err(Error::parse("bukan CREATE"));
    }
    p.eat_words(&["OR", "REPLACE"]);
    p.eat_word("TEMPORARY");
    if !p.eat_word("TABLE") {
        return Err(Error::parse("bukan CREATE TABLE"));
    }
    p.eat_words(&["IF", "NOT", "EXISTS"]);
    let (_, name) = p.qualified()?;
    if p.is_word("LIKE") || p.is_word("AS") || p.is_word("SELECT") {
        return Ok(None);
    }
    let body = p.group()?;
    let mut t = Table { name, ..Default::default() };
    let mut fk_seq = 0;
    for def in split_commas(body) {
        parse_def(def, &mut t, &mut fk_seq)
            .map_err(|e| Error::parse(format!("tabel `{}`: {e}", t.name)))?;
    }
    parse_table_options(&mut p, &mut t.options)?;
    Ok(Some(t))
}

fn parse_def(def: &[Tok], t: &mut Table, fk_seq: &mut usize) -> Result<()> {
    let mut p = P::new(def);
    let mut cname: Option<String> = None;
    if p.eat_word("CONSTRAINT")
        && !(p.is_word("PRIMARY") || p.is_word("UNIQUE") || p.is_word("FOREIGN") || p.is_word("CHECK")) {
            cname = Some(p.name()?);
        }
    if p.eat_word("PRIMARY") {
        p.eat_word("KEY");
        let mut idx = parse_index_rest(&mut p, IndexKind::Primary)?;
        idx.name = "PRIMARY".into();
        t.primary_key = Some(idx);
        return Ok(());
    }
    if p.eat_word("UNIQUE") {
        let _ = p.eat_word("KEY") || p.eat_word("INDEX");
        let mut idx = parse_index_rest(&mut p, IndexKind::Unique)?;
        if idx.name.is_empty() {
            idx.name = cname.unwrap_or_else(|| default_index_name(&idx, t));
        }
        t.indexes.push(idx);
        return Ok(());
    }
    if p.is_word("KEY") || p.is_word("INDEX") {
        p.next();
        let mut idx = parse_index_rest(&mut p, IndexKind::Index)?;
        if idx.name.is_empty() {
            idx.name = default_index_name(&idx, t);
        }
        t.indexes.push(idx);
        return Ok(());
    }
    if p.is_word("FULLTEXT") || p.is_word("SPATIAL") {
        let kind = if p.is_word("FULLTEXT") { IndexKind::Fulltext } else { IndexKind::Spatial };
        p.next();
        let _ = p.eat_word("KEY") || p.eat_word("INDEX");
        let mut idx = parse_index_rest(&mut p, kind)?;
        if idx.name.is_empty() {
            idx.name = default_index_name(&idx, t);
        }
        t.indexes.push(idx);
        return Ok(());
    }
    if p.eat_word("FOREIGN") {
        p.eat_word("KEY");
        let mut idx_name = None;
        if !p.is_punct("(") {
            idx_name = Some(p.name()?);
        }
        let cols = name_list(p.group()?)?;
        if !p.eat_word("REFERENCES") {
            return Err(Error::parse("FOREIGN KEY tanpa REFERENCES"));
        }
        let (ref_schema, ref_table) = p.qualified()?;
        let ref_cols = name_list(p.group()?)?;
        let mut fk = ForeignKey {
            name: String::new(),
            columns: cols,
            ref_schema,
            ref_table,
            ref_columns: ref_cols,
            on_delete: None,
            on_update: None,
        };
        while !p.done() {
            if p.eat_word("MATCH") {
                p.next();
            } else if p.eat_words(&["ON", "DELETE"]) {
                fk.on_delete = Some(ref_action(&mut p)?);
            } else if p.eat_words(&["ON", "UPDATE"]) {
                fk.on_update = Some(ref_action(&mut p)?);
            } else {
                p.next();
            }
        }
        *fk_seq += 1;
        fk.name = cname.or(idx_name).unwrap_or_else(|| format!("{}_ibfk_{}", t.name, fk_seq));
        t.foreign_keys.push(fk);
        return Ok(());
    }
    if p.eat_word("CHECK") {
        let expr = canonical_expr(p.group()?);
        let enforced = !p.eat_words(&["NOT", "ENFORCED"]);
        t.checks.push(Check { name: cname, expr, enforced });
        return Ok(());
    }
    if p.is_word("PERIOD") {
        t.options.other.insert("PERIOD".into(), render_tokens(def));
        return Ok(());
    }
    if cname.is_some() {
        return Err(Error::parse(format!("CONSTRAINT tidak dikenal: {}", render_tokens(def))));
    }
    let col = parse_column(&mut p, t)?;
    t.columns.push(col);
    Ok(())
}

fn default_index_name(idx: &Index, t: &Table) -> String {
    let base = idx
        .parts
        .first()
        .and_then(|p| p.column.clone())
        .unwrap_or_else(|| "functional_index".into());
    let mut name = base.clone();
    let mut n = 2;
    while t.indexes.iter().any(|i| i.name.eq_ignore_ascii_case(&name)) {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

fn ref_action(p: &mut P) -> Result<String> {
    let a = match p.next() {
        Some(Tok::Word(w)) => w.to_ascii_uppercase(),
        t => return Err(Error::parse(format!("aksi FK tidak valid: {t:?}"))),
    };
    if a == "SET" || a == "NO" {
        let b = match p.next() {
            Some(Tok::Word(w)) => w.to_ascii_uppercase(),
            _ => String::new(),
        };
        return Ok(format!("{a} {b}"));
    }
    Ok(a)
}

fn name_list(toks: &[Tok]) -> Result<Vec<String>> {
    split_commas(toks)
        .into_iter()
        .map(|part| {
            part.first()
                .and_then(|t| t.name())
                .map(|s| s.to_string())
                .ok_or_else(|| Error::parse("daftar kolom tidak valid"))
        })
        .collect()
}

fn parse_index_rest(p: &mut P, kind: IndexKind) -> Result<Index> {
    let mut idx = Index {
        name: String::new(),
        kind,
        parts: vec![],
        using: None,
        comment: None,
        invisible: false,
        extra: vec![],
    };
    if !p.is_punct("(") && !p.is_word("USING") {
        idx.name = p.name()?;
    }
    if p.eat_word("USING") {
        idx.using = Some(p.name()?.to_ascii_uppercase());
    }
    for part in split_commas(p.group()?) {
        let mut q = P::new(part);
        let mut ip = IndexPart { column: None, expr: None, prefix: None, desc: false };
        if q.is_punct("(") {
            ip.expr = Some(canonical_expr(q.group()?));
        } else {
            ip.column = Some(q.name()?);
            if q.is_punct("(") {
                let g = q.group()?;
                if let Some(Tok::Num(n)) = g.first() {
                    ip.prefix = n.parse().ok();
                }
            }
        }
        if q.eat_word("DESC") {
            ip.desc = true;
        }
        q.eat_word("ASC");
        idx.parts.push(ip);
    }
    while let Some(tok) = p.peek() {
        if p.eat_word("USING") {
            idx.using = Some(p.name()?.to_ascii_uppercase());
        } else if p.eat_word("COMMENT") {
            p.opt_eq();
            idx.comment = Some(p.string()?);
        } else if p.eat_word("INVISIBLE") || p.eat_word("IGNORED") {
            idx.invisible = true;
        } else if p.eat_word("VISIBLE") || p.eat_words(&["NOT", "IGNORED"]) {
        } else if p.eat_words(&["WITH", "PARSER"]) {
            idx.extra.push(format!("WITH PARSER {}", p.name()?));
        } else if let Tok::Word(w) = tok {
            p.next();
            p.opt_eq();
            let v = p.next().map(tok_text).unwrap_or_default();
            idx.extra.push(format!("{}={}", w.to_ascii_uppercase(), v));
        } else {
            p.next();
        }
    }
    Ok(idx)
}

fn parse_column(p: &mut P, t: &mut Table) -> Result<Column> {
    let name = p.name()?;
    let mut col = Column { name: name.clone(), nullable: true, ..Default::default() };
    col.data_type = parse_type(p)?;
    while !p.done() {
        if p.eat_words(&["NOT", "NULL"]) {
            col.nullable = false;
        } else if p.eat_word("NULL") {
            col.nullable = true;
        } else if p.eat_word("DEFAULT") {
            col.default = Some(parse_default(p)?);
        } else if p.eat_words(&["ON", "UPDATE"]) {
            col.on_update = Some(parse_on_update(p)?);
        } else if p.eat_word("AUTO_INCREMENT") {
            col.auto_increment = true;
        } else if p.eat_word("COMMENT") {
            col.comment = Some(p.string()?);
        } else if p.eat_words(&["CHARACTER", "SET"]) || p.eat_word("CHARSET") {
            col.charset = Some(p.name()?.to_ascii_lowercase());
        } else if p.eat_word("COLLATE") {
            col.collation = Some(p.name()?.to_ascii_lowercase());
        } else if p.eat_words(&["GENERATED", "ALWAYS"]) || p.is_word("AS") {
            p.eat_word("AS");
            let expr = canonical_expr(p.group()?);
            let stored = p.eat_word("STORED") || p.eat_word("PERSISTENT");
            p.eat_word("VIRTUAL");
            col.generated = Some(Generated { expr, stored });
        } else if p.eat_word("INVISIBLE") {
            col.invisible = true;
        } else if p.eat_word("VISIBLE") {
        } else if p.eat_word("UNIQUE") {
            p.eat_word("KEY");
            t.indexes.push(Index {
                name: name.clone(),
                kind: IndexKind::Unique,
                parts: vec![IndexPart { column: Some(name.clone()), expr: None, prefix: None, desc: false }],
                using: None,
                comment: None,
                invisible: false,
                extra: vec![],
            });
        } else if p.eat_words(&["PRIMARY", "KEY"]) || p.eat_word("KEY") {
            t.primary_key = Some(Index {
                name: "PRIMARY".into(),
                kind: IndexKind::Primary,
                parts: vec![IndexPart { column: Some(name.clone()), expr: None, prefix: None, desc: false }],
                using: None,
                comment: None,
                invisible: false,
                extra: vec![],
            });
            col.nullable = false;
        } else if p.eat_word("CHECK") {
            col.check = Some(canonical_expr(p.group()?));
        } else if p.eat_word("REFERENCES") {
            // MySQL mengabaikan REFERENCES inline.
            while !p.done() {
                p.next();
            }
        } else if p.eat_words(&["WITH", "SYSTEM", "VERSIONING"]) {
            col.extra.push("WITH SYSTEM VERSIONING".into());
        } else if p.eat_words(&["WITHOUT", "SYSTEM", "VERSIONING"]) {
            col.extra.push("WITHOUT SYSTEM VERSIONING".into());
        } else if p.is_word("COLUMN_FORMAT") || p.is_word("STORAGE") || p.is_word("SRID") {
            let k = tok_text(p.next().unwrap()).to_ascii_uppercase();
            p.opt_eq();
            let v = p.next().map(tok_text).unwrap_or_default();
            col.extra.push(format!("{k} {v}"));
        } else {
            let tok = p.next().unwrap();
            let mut s = tok_text(tok);
            if let Tok::Word(_) = tok {
                if p.eat_punct("=") {
                    s = format!("{}={}", s.to_ascii_uppercase(), p.next().map(tok_text).unwrap_or_default());
                } else {
                    s = s.to_ascii_uppercase();
                }
            }
            col.extra.push(s);
        }
    }
    Ok(col)
}

fn parse_type(p: &mut P) -> Result<String> {
    let mut base = match p.next() {
        Some(Tok::Word(w)) => w.to_ascii_lowercase(),
        Some(Tok::Ident(w)) => w.to_ascii_lowercase(),
        t => return Err(Error::parse(format!("butuh tipe data, dapat {t:?}"))),
    };
    match base.as_str() {
        "double" => {
            p.eat_word("PRECISION");
        }
        "national" => {
            base = match p.next() {
                Some(Tok::Word(w)) => w.to_ascii_lowercase(),
                _ => "char".into(),
            };
        }
        "character" | "char" if p.is_word("VARYING") => {
            p.next();
            base = "varchar".into();
        }
        "long" => {
            if p.eat_word("VARBINARY") {
                base = "mediumblob".into();
            } else {
                p.eat_word("VARCHAR");
                base = "mediumtext".into();
            }
        }
        _ => {}
    }
    let mut s = base;
    if p.is_punct("(") {
        let g = p.group()?;
        s.push('(');
        for t in g {
            match t {
                Tok::Str(v) => s.push_str(&quote_str(v)),
                other => s.push_str(&tok_text(other)),
            }
        }
        s.push(')');
    }
    loop {
        if p.eat_word("UNSIGNED") {
            s.push_str(" unsigned");
        } else if p.eat_word("SIGNED") {
        } else if p.eat_word("ZEROFILL") {
            s.push_str(" zerofill");
        } else if p.eat_word("BINARY") {
            // `varchar(10) binary` lama = collation _bin
            s.push_str(" binary");
        } else {
            break;
        }
    }
    Ok(s)
}

fn is_now_fn(w: &str) -> bool {
    ["CURRENT_TIMESTAMP", "NOW", "LOCALTIME", "LOCALTIMESTAMP"]
        .iter()
        .any(|k| w.eq_ignore_ascii_case(k))
}

fn parse_now(p: &mut P) -> Result<Option<u8>> {
    let mut prec = None;
    if p.is_punct("(") {
        let g = p.group()?;
        if let Some(Tok::Num(n)) = g.first() {
            prec = n.parse().ok().filter(|&v: &u8| v > 0);
        }
    }
    Ok(prec)
}

fn parse_on_update(p: &mut P) -> Result<String> {
    match p.next() {
        Some(Tok::Word(w)) if is_now_fn(w) => {
            let prec = parse_now(p)?;
            Ok(match prec {
                Some(n) => format!("CURRENT_TIMESTAMP({n})"),
                None => "CURRENT_TIMESTAMP".into(),
            })
        }
        Some(t) => Ok(tok_text(t)),
        None => Err(Error::parse("ON UPDATE kosong")),
    }
}

fn parse_default(p: &mut P) -> Result<DefaultValue> {
    let tok = p.next().ok_or_else(|| Error::parse("DEFAULT kosong"))?;
    Ok(match tok {
        Tok::Punct(x) if x == "(" => {
            p.i -= 1;
            let inner = p.group()?;
            literal_from_tokens(inner).unwrap_or_else(|| DefaultValue::Expr(canonical_expr(inner)))
        }
        Tok::Punct(x) if x == "-" || x == "+" => match p.next() {
            Some(Tok::Num(n)) => DefaultValue::Num(if x == "-" { format!("-{n}") } else { n.clone() }),
            t => return Err(Error::parse(format!("DEFAULT tidak valid: {x}{t:?}"))),
        },
        Tok::Num(n) => DefaultValue::Num(n.clone()),
        Tok::Str(s) => DefaultValue::Str(s.clone()),
        Tok::Hex(h) => DefaultValue::Hex(h.clone()),
        Tok::Bits(b) => DefaultValue::Bits(b.clone()),
        Tok::Word(w) if w.starts_with('_') && matches!(p.peek(), Some(Tok::Str(_)) | Some(Tok::Hex(_))) => {
            match p.next() {
                Some(Tok::Str(s)) => DefaultValue::Str(s.clone()),
                Some(Tok::Hex(h)) => DefaultValue::Hex(h.clone()),
                _ => unreachable!(),
            }
        }
        Tok::Word(w) if w.eq_ignore_ascii_case("NULL") => DefaultValue::Null,
        Tok::Word(w) if w.eq_ignore_ascii_case("TRUE") => DefaultValue::Num("1".into()),
        Tok::Word(w) if w.eq_ignore_ascii_case("FALSE") => DefaultValue::Num("0".into()),
        Tok::Word(w) if is_now_fn(w) => DefaultValue::CurrentTimestamp(parse_now(p)?),
        Tok::Word(_) if p.is_punct("(") => {
            let start = p.i - 1;
            p.group()?;
            DefaultValue::Expr(render_tokens(&p.t[start..p.i]))
        }
        other => DefaultValue::Expr(tok_text(other)),
    })
}

/// `(0)`, `('x')`, `(-1)` -> literal. Selain itu None.
fn literal_from_tokens(toks: &[Tok]) -> Option<DefaultValue> {
    match toks {
        [Tok::Num(n)] => Some(DefaultValue::Num(n.clone())),
        [Tok::Punct(m), Tok::Num(n)] if m == "-" => Some(DefaultValue::Num(format!("-{n}"))),
        [Tok::Str(s)] => Some(DefaultValue::Str(s.clone())),
        [Tok::Word(w), Tok::Str(s)] if w.starts_with('_') => Some(DefaultValue::Str(s.clone())),
        [Tok::Word(w)] if w.eq_ignore_ascii_case("NULL") => Some(DefaultValue::Null),
        [Tok::Word(w)] if is_now_fn(w) => Some(DefaultValue::CurrentTimestamp(None)),
        [Tok::Word(w), Tok::Punct(a), Tok::Punct(b)] if is_now_fn(w) && a == "(" && b == ")" => {
            Some(DefaultValue::CurrentTimestamp(None))
        }
        [Tok::Word(w), Tok::Punct(a), Tok::Num(n), Tok::Punct(b)] if is_now_fn(w) && a == "(" && b == ")" => {
            Some(DefaultValue::CurrentTimestamp(n.parse().ok().filter(|&v: &u8| v > 0)))
        }
        _ => None,
    }
}

fn parse_table_options(p: &mut P, o: &mut TableOptions) -> Result<()> {
    while !p.done() {
        if p.eat_punct(",") || p.eat_punct(";") {
            continue;
        }
        if p.is_word("PARTITION") {
            o.partition = Some(render_tokens(&p.t[p.i..]));
            p.i = p.t.len();
            break;
        }
        if p.eat_words(&["WITH", "SYSTEM", "VERSIONING"]) {
            o.other.insert("WITH SYSTEM VERSIONING".into(), String::new());
            continue;
        }
        p.eat_word("DEFAULT");
        if p.eat_words(&["CHARACTER", "SET"]) || p.eat_word("CHARSET") {
            p.opt_eq();
            o.charset = Some(p.name()?.to_ascii_lowercase());
        } else if p.eat_word("COLLATE") {
            p.opt_eq();
            o.collation = Some(p.name()?.to_ascii_lowercase());
        } else if p.eat_word("ENGINE") || p.eat_word("TYPE") {
            p.opt_eq();
            o.engine = Some(p.name()?);
        } else if p.eat_word("COMMENT") {
            p.opt_eq();
            o.comment = Some(p.string()?);
        } else if p.eat_word("AUTO_INCREMENT") {
            p.opt_eq();
            if let Some(Tok::Num(n)) = p.next() {
                o.auto_increment = n.parse().ok();
            }
        } else if p.eat_word("ROW_FORMAT") {
            p.opt_eq();
            o.row_format = Some(p.name()?.to_ascii_uppercase());
        } else if let Some(Tok::Word(w)) = p.peek() {
            p.next();
            p.opt_eq();
            let v = if p.is_punct("(") {
                format!("({})", render_tokens(p.group()?))
            } else {
                p.next().map(tok_text).unwrap_or_default()
            };
            o.other.insert(w.to_ascii_uppercase(), v);
        } else {
            p.next();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ct(sql: &str) -> Table {
        match classify(sql.as_bytes()).unwrap() {
            Stmt::CreateTable(t) => t,
            other => panic!("bukan create table: {other:?}"),
        }
    }

    #[test]
    fn parses_mysql8_table() {
        let t = ct(r#"CREATE TABLE `orders` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `customer_id` int NOT NULL,
  `note` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL COMMENT 'catatan',
  `status` enum('new','paid','it''s') NOT NULL DEFAULT 'new',
  `total` decimal(12,2) NOT NULL DEFAULT '0.00',
  `created_at` timestamp NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  `code` varchar(20) GENERATED ALWAYS AS (concat(`id`,_utf8mb4'-x')) VIRTUAL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_code` (`code`),
  KEY `idx_cust` (`customer_id`,`created_at` DESC),
  KEY `idx_note` (`note`(10)),
  CONSTRAINT `fk_cust` FOREIGN KEY (`customer_id`) REFERENCES `customers` (`id`) ON DELETE CASCADE,
  CONSTRAINT `chk_total` CHECK ((`total` >= 0))
) ENGINE=InnoDB AUTO_INCREMENT=42 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='Pesanan'"#);
        assert_eq!(t.name, "orders");
        assert_eq!(t.columns.len(), 7);
        assert_eq!(t.columns[0].data_type, "bigint unsigned");
        assert!(t.columns[0].auto_increment && !t.columns[0].nullable);
        assert_eq!(t.columns[2].collation.as_deref(), Some("utf8mb4_unicode_ci"));
        assert_eq!(t.columns[2].comment.as_deref(), Some("catatan"));
        assert_eq!(t.columns[3].data_type, "enum('new','paid','it\\'s')");
        assert_eq!(t.columns[4].default, Some(DefaultValue::Str(b"0.00".to_vec())));
        assert_eq!(t.columns[5].default, Some(DefaultValue::CurrentTimestamp(None)));
        assert_eq!(t.columns[5].on_update.as_deref(), Some("CURRENT_TIMESTAMP"));
        assert!(t.columns[6].generated.is_some());
        assert_eq!(t.primary_key.as_ref().unwrap().parts[0].column.as_deref(), Some("id"));
        assert_eq!(t.indexes.len(), 3);
        assert!(t.indexes[1].parts[1].desc);
        assert_eq!(t.indexes[2].parts[0].prefix, Some(10));
        assert_eq!(t.foreign_keys[0].on_delete.as_deref(), Some("CASCADE"));
        assert_eq!(t.checks.len(), 1);
        assert_eq!(t.options.engine.as_deref(), Some("InnoDB"));
        assert_eq!(t.options.auto_increment, Some(42));
        assert_eq!(t.options.collation.as_deref(), Some("utf8mb4_0900_ai_ci"));
        assert_eq!(t.options.comment.as_deref(), Some("Pesanan"));
    }

    #[test]
    fn parses_mariadb_table() {
        let t = ct(r#"CREATE TABLE `pasien` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `nama` varchar(100) DEFAULT NULL,
  `aktif` tinyint(1) NOT NULL DEFAULT 1,
  `data` longtext CHARACTER SET utf8mb4 COLLATE utf8mb4_bin DEFAULT NULL CHECK (json_valid(`data`)),
  `dibuat` datetime NOT NULL DEFAULT current_timestamp(),
  `flag` bit(1) DEFAULT b'0',
  PRIMARY KEY (`id`) USING BTREE
) ENGINE=InnoDB DEFAULT CHARSET=latin1 COLLATE=latin1_swedish_ci"#);
        assert_eq!(t.columns[0].data_type, "int(11)");
        assert_eq!(t.columns[1].default, Some(DefaultValue::Null));
        assert_eq!(t.columns[2].default, Some(DefaultValue::Num("1".into())));
        assert_eq!(t.columns[3].check.as_deref(), Some("json_valid(`data`)"));
        assert_eq!(t.columns[4].default, Some(DefaultValue::CurrentTimestamp(None)));
        assert_eq!(t.columns[5].default, Some(DefaultValue::Bits("0".into())));
        assert_eq!(t.primary_key.as_ref().unwrap().using.as_deref(), Some("BTREE"));
    }

    #[test]
    fn classifies_other_statements() {
        assert!(matches!(classify(b"INSERT INTO `db`.`t` VALUES (1)").unwrap(), Stmt::Insert(n) if n == "t"));
        assert!(matches!(classify(b"/*!40000 ALTER TABLE `t` DISABLE KEYS */").unwrap(), Stmt::Ignore));
        assert!(matches!(classify(b"DROP TABLE IF EXISTS `a`, `b`").unwrap(), Stmt::DropTables(v) if v == vec!["a","b"]));
        match classify(b"/*!50001 CREATE ALGORITHM=UNDEFINED */ /*!50013 DEFINER=`root`@`%` SQL SECURITY DEFINER */ /*!50001 VIEW `v1` AS select 1 AS `a` */").unwrap() {
            Stmt::Skipped(o) => assert_eq!((o.kind.as_str(), o.name.as_str()), ("view", "v1")),
            other => panic!("{other:?}"),
        }
        match classify(b"/*!50003 CREATE*/ /*!50017 DEFINER=`root`@`localhost`*/ /*!50003 TRIGGER `trg` BEFORE INSERT ON `t` FOR EACH ROW SET NEW.a = 1 */").unwrap() {
            Stmt::Skipped(o) => assert_eq!((o.kind.as_str(), o.name.as_str()), ("trigger", "trg")),
            other => panic!("{other:?}"),
        }
    }
}
