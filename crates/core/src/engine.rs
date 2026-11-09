//! Orkestrasi: buka dua sisi (file/live), jalankan schema diff + data diff.

use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::data_diff::{diff_table_data, TableDataDiff};
use crate::dump::{open_dump_side, DumpSource};
use crate::model::Schema;
use crate::normalize::CompareOptions;
use crate::progress::Progress;
use crate::row::{EmptySource, RowFn, RowSource};
use crate::schema_diff::{diff_schemas, pair_tables, TableSchemaDiff};
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SideSpec {
    #[serde(rename_all = "camelCase")]
    File { schema_path: PathBuf, data_path: Option<PathBuf> },
    #[cfg(feature = "live")]
    #[serde(rename_all = "camelCase")]
    Live { params: crate::live::ConnParams, database: String },
}

impl SideSpec {
    pub fn label(&self) -> String {
        match self {
            SideSpec::File { schema_path, data_path } => {
                let f = |p: &PathBuf| p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                match data_path {
                    Some(d) if d != schema_path => format!("{} + {}", f(schema_path), f(d)),
                    _ => f(schema_path),
                }
            }
            #[cfg(feature = "live")]
            SideSpec::Live { params, database } => format!("{}@{}:{}/{}", params.user, params.host, params.port, database),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CompareRequest {
    pub a: SideSpec,
    pub b: SideSpec,
    pub compare_schema: bool,
    pub compare_data: bool,
    pub options: CompareOptions,
    /// Batasi data diff ke tabel ini (huruf kecil). None = semua.
    pub data_tables: Option<Vec<String>>,
}

impl Default for CompareRequest {
    fn default() -> Self {
        let empty = SideSpec::File { schema_path: PathBuf::new(), data_path: None };
        CompareRequest {
            a: empty.clone(),
            b: empty,
            compare_schema: true,
            compare_data: true,
            options: CompareOptions::default(),
            data_tables: None,
        }
    }
}

pub enum Source {
    Dump(DumpSource),
    #[cfg(feature = "live")]
    Live(Box<crate::live::LiveSource>),
}

impl RowSource for Source {
    fn has_data(&self) -> bool {
        match self {
            Source::Dump(d) => d.has_data(),
            #[cfg(feature = "live")]
            Source::Live(l) => l.has_data(),
        }
    }
    fn for_each_row(&mut self, t: &crate::model::Table, f: &mut RowFn<'_>) -> Result<()> {
        match self {
            Source::Dump(d) => d.for_each_row(t, f),
            #[cfg(feature = "live")]
            Source::Live(l) => l.for_each_row(t, f),
        }
    }
}

pub struct OpenedSide {
    pub schema: Schema,
    pub source: Source,
    pub label: String,
    pub server_version: Option<String>,
}

pub fn open_side(spec: &SideSpec, progress: &dyn Progress, label: &str) -> Result<OpenedSide> {
    match spec {
        SideSpec::File { schema_path, data_path } => {
            if schema_path.as_os_str().is_empty() {
                return Err(Error::Other(format!("File sisi {label} belum dipilih")));
            }
            let side = open_dump_side(schema_path, data_path.as_deref(), progress, label)?;
            Ok(OpenedSide {
                schema: side.schema,
                source: Source::Dump(DumpSource::new(side.data)),
                label: spec.label(),
                server_version: None,
            })
        }
        #[cfg(feature = "live")]
        SideSpec::Live { params, database } => {
            let mut live = crate::live::LiveSource::connect(params, database)?;
            let schema = live.load_schema(progress)?;
            let v = live.version.clone();
            Ok(OpenedSide { schema, source: Source::Live(Box::new(live)), label: spec.label(), server_version: Some(v) })
        }
    }
}

#[derive(Debug, Default)]
pub struct CompareResult {
    pub label_a: String,
    pub label_b: String,
    pub version_a: Option<String>,
    pub version_b: Option<String>,
    pub schema_a: Schema,
    pub schema_b: Schema,
    pub schema_diff: Vec<TableSchemaDiff>,
    pub data: Vec<TableDataDiff>,
    pub data_skipped: Option<String>,
    pub data_errors: Vec<(String, String)>,
    pub warnings: Vec<String>,
    pub elapsed_ms: u128,
}

pub fn run_compare(req: &CompareRequest, progress: &dyn Progress) -> Result<CompareResult> {
    let t0 = Instant::now();
    let mut a = open_side(&req.a, progress, "A")?;
    progress.check()?;
    let mut b = open_side(&req.b, progress, "B")?;
    progress.check()?;

    let mut res = CompareResult {
        label_a: a.label.clone(),
        label_b: b.label.clone(),
        version_a: a.server_version.clone(),
        version_b: b.server_version.clone(),
        ..Default::default()
    };
    for w in &a.schema.warnings {
        res.warnings.push(format!("A: {w}"));
    }
    for w in &b.schema.warnings {
        res.warnings.push(format!("B: {w}"));
    }
    res.schema_diff = diff_schemas(&a.schema, &b.schema, &req.options);

    if req.compare_data {
        if !a.source.has_data() {
            res.data_skipped = Some("Sisi A tidak berisi data (hanya schema). Pilih file data untuk A.".into());
        } else if !b.source.has_data() {
            res.data_skipped = Some("Sisi B tidak berisi data (hanya schema). Pilih file data untuk B.".into());
        } else {
            let pairs: Vec<_> = pair_tables(&a.schema, &b.schema, &req.options)
                .into_iter()
                .filter_map(|(ta, tb)| tb.map(|tb| (ta.cloned(), tb.clone())))
                .filter(|(_, tb)| {
                    req.data_tables.as_ref().is_none_or(|v| v.iter().any(|n| n.eq_ignore_ascii_case(&tb.name)))
                })
                .collect();
            let total = pairs.len() as u64;
            for (i, (ta, tb)) in pairs.iter().enumerate() {
                progress.check()?;
                progress.report("Membandingkan data", &tb.name, i as u64, total);
                let r = match ta {
                    Some(ta) => diff_table_data(&mut a.source, &mut b.source, ta, tb, progress),
                    None => diff_table_data(&mut EmptySource, &mut b.source, tb, tb, progress),
                };
                match r {
                    Ok(d) => res.data.push(d),
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(e) => res.data_errors.push((tb.name.clone(), e.to_string())),
                }
            }
            progress.report("Membandingkan data", "", total, total);
        }
    }
    res.schema_a = a.schema;
    res.schema_b = b.schema;
    res.elapsed_ms = t0.elapsed().as_millis();
    Ok(res)
}
