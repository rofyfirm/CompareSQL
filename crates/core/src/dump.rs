//! Sumber dari file dump: scan sekali (schema + indeks offset INSERT),
//! lalu baca baris per tabel dengan seek.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ddl::{classify, Stmt};
use crate::lexer::{StatementReader, Tok, Tokenizer};
use crate::model::{Schema, Table, TypeClass, type_class};
use crate::normalize::canonicalize_schema;
use crate::progress::Progress;
use crate::row::*;
use crate::value::{bytes_to_decimal, parse_values};
use crate::{Error, Result};

#[derive(Debug, Default)]
pub struct DumpIndex {
    pub path: PathBuf,
    pub schema: Schema,
    /// Nama tabel (persis seperti di file) -> (offset, panjang) tiap INSERT.
    pub inserts: HashMap<String, Vec<(u64, u64)>>,
    pub bytes: u64,
    pub statements: u64,
    pub insert_statements: u64,
}

impl DumpIndex {
    pub fn has_data(&self) -> bool {
        self.insert_statements > 0
    }
    fn ranges(&self, table: &str) -> Option<&Vec<(u64, u64)>> {
        self.inserts.get(table).or_else(|| {
            self.inserts
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(table))
                .map(|(_, v)| v)
        })
    }
}

/// Scan file dump: parse CREATE TABLE, catat posisi INSERT.
pub fn scan_dump(path: &Path, progress: &dyn Progress, label: &str) -> Result<DumpIndex> {
    let file = File::open(path)?;
    let total = file.metadata()?.len();
    let mut rd = StatementReader::new(BufReader::with_capacity(1 << 20, file));
    let mut idx = DumpIndex { path: path.to_path_buf(), bytes: total, ..Default::default() };
    let mut unsupported: HashMap<String, u64> = HashMap::new();
    let mut last_report = 0u64;
    let phase = format!("Membaca {label}");
    progress.report(&phase, &file_name(path), 0, total);

    while let Some(st) = rd.next_statement()? {
        idx.statements += 1;
        let pos = rd.position();
        if pos - last_report > 8 << 20 {
            progress.check()?;
            progress.report(&phase, &file_name(path), pos, total);
            last_report = pos;
        }
        match classify(&st.bytes) {
            Ok(Stmt::CreateTable(t)) => idx.schema.upsert(t),
            Ok(Stmt::DropTables(names)) => {
                for n in names {
                    idx.schema.remove(&n);
                }
            }
            Ok(Stmt::Insert(name)) => {
                idx.insert_statements += 1;
                idx.inserts.entry(name).or_default().push((st.offset, st.bytes.len() as u64));
            }
            Ok(Stmt::Skipped(o)) => {
                if !idx.schema.skipped.iter().any(|x| x.kind == o.kind && x.name == o.name) {
                    idx.schema.skipped.push(o);
                }
            }
            Ok(Stmt::Unsupported(what)) => *unsupported.entry(what).or_default() += 1,
            Ok(Stmt::Ignore) => {}
            Err(e) => idx
                .schema
                .warnings
                .push(format!("{}: statement di offset {} dilewati: {e}", file_name(path), st.offset)),
        }
    }
    for (what, n) in unsupported {
        idx.schema.warnings.push(format!("{}: {n}x {what} tidak didukung, dilewati", file_name(path)));
    }
    canonicalize_schema(&mut idx.schema);
    progress.report(&phase, &file_name(path), total, total);
    Ok(idx)
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Satu sisi perbandingan dari file: schema (+ data di file lain bila ada).
pub struct DumpSide {
    pub schema: Schema,
    pub data: Arc<DumpIndex>,
}

pub fn open_dump_side(
    schema_path: &Path,
    data_path: Option<&Path>,
    progress: &dyn Progress,
    label: &str,
) -> Result<DumpSide> {
    let first = scan_dump(schema_path, progress, label)?;
    match data_path {
        Some(dp) if dp != schema_path => {
            let data = scan_dump(dp, progress, label)?;
            let mut schema = first.schema;
            for t in &data.schema.tables {
                if schema.table(&t.name).is_none() {
                    schema.tables.push(t.clone());
                }
            }
            schema.warnings.extend(data.schema.warnings.iter().cloned());
            Ok(DumpSide { schema, data: Arc::new(data) })
        }
        _ => {
            let schema = first.schema.clone();
            Ok(DumpSide { schema, data: Arc::new(first) })
        }
    }
}

/// Header INSERT: daftar kolom (bila ada) dan posisi setelah VALUES.
pub fn parse_insert_header(s: &[u8]) -> Result<(Option<Vec<String>>, usize)> {
    let mut tz = Tokenizer::new(s);
    let mut seen_name = false;
    let mut cols: Option<Vec<String>> = None;
    while let Some(t) = tz.next_tok() {
        match &t {
            Tok::Word(w) if w.eq_ignore_ascii_case("VALUES") || w.eq_ignore_ascii_case("VALUE") => {
                return Ok((cols, tz.pos()));
            }
            Tok::Word(w) if w.eq_ignore_ascii_case("SET") || w.eq_ignore_ascii_case("SELECT") => {
                return Err(Error::parse("INSERT ... SET/SELECT tidak didukung"));
            }
            Tok::Ident(_) => seen_name = true,
            Tok::Word(w)
                if !["INSERT", "REPLACE", "INTO", "IGNORE", "LOW_PRIORITY", "DELAYED", "HIGH_PRIORITY"]
                    .iter()
                    .any(|k| w.eq_ignore_ascii_case(k)) =>
            {
                seen_name = true
            }
            Tok::Punct(p) if p == "(" && seen_name => {
                let mut v = vec![];
                for t in tz.by_ref() {
                    match t {
                        Tok::Ident(n) | Tok::Word(n) => v.push(n),
                        Tok::Punct(p) if p == ")" => break,
                        _ => {}
                    }
                }
                cols = Some(v);
            }
            _ => {}
        }
    }
    Err(Error::parse("INSERT tanpa VALUES"))
}

pub struct DumpSource {
    index: Arc<DumpIndex>,
    file: Option<File>,
    buf: Vec<u8>,
    row: RowBuf,
}

impl DumpSource {
    pub fn new(index: Arc<DumpIndex>) -> Self {
        DumpSource { index, file: None, buf: Vec::new(), row: RowBuf::default() }
    }
    pub fn index(&self) -> &DumpIndex {
        &self.index
    }
}

impl RowSource for DumpSource {
    fn has_data(&self) -> bool {
        self.index.has_data()
    }

    fn for_each_row(&mut self, table: &Table, f: &mut RowFn<'_>) -> Result<()> {
        let Some(ranges) = self.index.ranges(&table.name).cloned() else {
            return Ok(());
        };
        if self.file.is_none() {
            self.file = Some(File::open(&self.index.path)?);
        }
        let file = self.file.as_mut().unwrap();
        let ncols = table.columns.len();
        let bit_cols: Vec<usize> = table
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| type_class(&c.data_type) == TypeClass::Bit)
            .map(|(i, _)| i)
            .collect();
        let non_generated: Vec<usize> =
            table.columns.iter().enumerate().filter(|(_, c)| c.generated.is_none()).map(|(i, _)| i).collect();

        for (off, len) in ranges {
            file.seek(SeekFrom::Start(off))?;
            self.buf.resize(len as usize, 0);
            file.read_exact(&mut self.buf)?;
            let (cols, vpos) = parse_insert_header(&self.buf)
                .map_err(|e| Error::parse(format!("tabel `{}`: {e}", table.name)))?;

            // map: kolom tabel -> indeks sel dalam tuple
            let mut map: Vec<Option<usize>> = vec![None; ncols];
            let explicit = cols.is_some();
            if let Some(cols) = &cols {
                for (ci, name) in cols.iter().enumerate() {
                    if let Some(ti) = table.column_index(name) {
                        map[ti] = Some(ci);
                    }
                }
            }
            let mut checked = explicit;
            let tname = &table.name;
            parse_values(&self.buf, vpos, &mut self.row, |row| {
                if !checked {
                    if row.len() == ncols {
                        for (i, m) in map.iter_mut().enumerate() {
                            *m = Some(i);
                        }
                    } else if row.len() == non_generated.len() {
                        for (ci, &ti) in non_generated.iter().enumerate() {
                            map[ti] = Some(ci);
                        }
                    } else {
                        return Err(Error::parse(format!(
                            "tabel `{tname}`: INSERT tanpa daftar kolom punya {} nilai, schema punya {ncols} kolom. \
                             Pastikan schema cocok atau dump dengan --complete-insert",
                            row.len()
                        )));
                    }
                    checked = true;
                }
                for &bc in &bit_cols {
                    if let Some(ci) = map[bc] {
                        if row.kind(ci) == KIND_HEX || row.kind(ci) == KIND_STR {
                            if let Some(b) = row.get(ci) {
                                let d = bytes_to_decimal(b);
                                row.replace(ci, &d, KIND_BITS);
                            }
                        }
                    }
                }
                f(&RowView { buf: row, map: &map })
            })
            .map_err(|e| match e {
                Error::Parse(m) if !m.starts_with("tabel") => Error::parse(format!("tabel `{}`: {m}", table.name)),
                other => other,
            })?;
        }
        Ok(())
    }
}
