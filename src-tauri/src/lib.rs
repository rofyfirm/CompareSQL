//! Shell Tauri: command untuk UI React. Logika ada di `selisih-core`.

mod connections;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use selisih_core::data_diff::{changed_columns, KeyMode, TableDataDiff};
use selisih_core::engine::{run_compare, CompareRequest, CompareResult, SideSpec};
use selisih_core::lexer::hex_encode;
use selisih_core::live::{ConnParams, ExecReport};
use selisih_core::model::{type_class, SkippedObject, Table, TypeClass};
use selisih_core::normalize::CompareOptions;
use selisih_core::progress::Progress;
use selisih_core::render::render_create_table;
use selisih_core::row::Cell;
use selisih_core::schema_diff::{Change, Status};
use selisih_core::script::{generate, ScriptInput, ScriptOptions};
use serde::{Deserialize, Serialize};
use similar::{DiffTag, TextDiff};
use tauri::{AppHandle, Emitter, State};

use connections::ConnectionProfile;

struct Session {
    req: CompareRequest,
    res: CompareResult,
    /// Target eksekusi bila A adalah DB live.
    a_live: Option<(ConnParams, String)>,
}

#[derive(Default)]
struct AppState {
    session: Mutex<Option<Arc<Session>>>,
    script: Mutex<Option<Arc<String>>>,
    cancel: Arc<AtomicBool>,
}

// ---------------------------------------------------------------------------
// Progress -> event "progress"

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    phase: String,
    detail: String,
    done: u64,
    total: u64,
}

struct Emit {
    app: AppHandle,
    cancel: Arc<AtomicBool>,
    last: Mutex<Instant>,
}

impl Emit {
    fn new(app: AppHandle, cancel: Arc<AtomicBool>) -> Self {
        Emit { app, cancel, last: Mutex::new(Instant::now() - Duration::from_secs(1)) }
    }
}

impl Progress for Emit {
    fn report(&self, phase: &str, detail: &str, done: u64, total: u64) {
        let mut last = self.last.lock().unwrap();
        if last.elapsed() < Duration::from_millis(80) && done != total {
            return;
        }
        *last = Instant::now();
        let _ = self.app.emit(
            "progress",
            ProgressEvent { phase: phase.into(), detail: detail.into(), done, total },
        );
    }
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Jalankan kerja berat di thread biasa (bukan thread runtime tokio),
/// karena sumber live membuat runtime sendiri.
async fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || std::thread::spawn(f).join().map_err(|_| "worker panic".to_string()))
        .await
        .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Compare

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SideInput {
    #[serde(rename_all = "camelCase")]
    File { schema_path: String, data_path: Option<String> },
    #[serde(rename_all = "camelCase")]
    Live { connection_id: String, database: String },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompareInput {
    a: SideInput,
    b: SideInput,
    compare_data: bool,
    options: CompareOptions,
}

fn to_spec(app: &AppHandle, s: SideInput) -> Result<(SideSpec, Option<(ConnParams, String)>), String> {
    Ok(match s {
        SideInput::File { schema_path, data_path } => (
            SideSpec::File {
                schema_path: schema_path.into(),
                data_path: data_path.filter(|d| !d.is_empty()).map(Into::into),
            },
            None,
        ),
        SideInput::Live { connection_id, database } => {
            if database.is_empty() {
                return Err("Pilih database".into());
            }
            let prof = connections::find(app, &connection_id)?;
            let params = connections::params(&prof, None);
            (SideSpec::Live { params: params.clone(), database: database.clone() }, Some((params, database)))
        }
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DataSummary {
    rows_a: u64,
    rows_b: u64,
    same: u64,
    changed: usize,
    only_a: usize,
    only_b: usize,
    key_mode: Option<KeyMode>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableSummary {
    name: String,
    name_a: Option<String>,
    name_b: Option<String>,
    status: Status,
    changes: Vec<Change>,
    data: Option<DataSummary>,
    data_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompareSummary {
    label_a: String,
    label_b: String,
    version_a: Option<String>,
    version_b: Option<String>,
    elapsed_ms: u128,
    data_compared: bool,
    data_skipped: Option<String>,
    warnings: Vec<String>,
    tables: Vec<TableSummary>,
    skipped_a: Vec<SkippedObject>,
    skipped_b: Vec<SkippedObject>,
    a_is_live: bool,
    a_database: Option<String>,
}

fn summarize(s: &Session) -> CompareSummary {
    let r = &s.res;
    let tables = r
        .schema_diff
        .iter()
        .map(|t| {
            let key = t.name_b.as_deref().unwrap_or(&t.name);
            let data = r.data.iter().find(|d| d.table.eq_ignore_ascii_case(key)).map(|d| DataSummary {
                rows_a: d.rows_a,
                rows_b: d.rows_b,
                same: d.same,
                changed: d.changed.len(),
                only_a: d.only_in_a.len(),
                only_b: d.only_in_b.len(),
                key_mode: d.key_mode,
                warnings: d.warnings.clone(),
            });
            let data_error = r.data_errors.iter().find(|(n, _)| n.eq_ignore_ascii_case(key)).map(|(_, e)| e.clone());
            TableSummary {
                name: t.name.clone(),
                name_a: t.name_a.clone(),
                name_b: t.name_b.clone(),
                status: t.status,
                changes: t.changes.clone(),
                data,
                data_error,
            }
        })
        .collect();
    CompareSummary {
        label_a: r.label_a.clone(),
        label_b: r.label_b.clone(),
        version_a: r.version_a.clone(),
        version_b: r.version_b.clone(),
        elapsed_ms: r.elapsed_ms,
        data_compared: s.req.compare_data && r.data_skipped.is_none(),
        data_skipped: r.data_skipped.clone(),
        warnings: r.warnings.clone(),
        tables,
        skipped_a: r.schema_a.skipped.clone(),
        skipped_b: r.schema_b.skipped.clone(),
        a_is_live: s.a_live.is_some(),
        a_database: s.a_live.as_ref().map(|(_, d)| d.clone()),
    }
}

#[tauri::command]
async fn compare(app: AppHandle, state: State<'_, AppState>, input: CompareInput) -> Result<CompareSummary, String> {
    let (a, a_live) = to_spec(&app, input.a)?;
    let (b, _) = to_spec(&app, input.b)?;
    let req = CompareRequest { a, b, compare_data: input.compare_data, options: input.options, ..Default::default() };
    state.cancel.store(false, Ordering::Relaxed);
    let cancel = state.cancel.clone();
    let app2 = app.clone();
    let req2 = req.clone();
    let res = off_runtime(move || run_compare(&req2, &Emit::new(app2, cancel))).await?.map_err(|e| e.to_string())?;
    let session = Arc::new(Session { req, res, a_live });
    let summary = summarize(&session);
    *state.session.lock().unwrap() = Some(session);
    *state.script.lock().unwrap() = None;
    Ok(summary)
}

#[tauri::command]
fn cancel(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Detail tabel

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffLine {
    kind: &'static str,
    a: Option<String>,
    b: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangedRowView {
    a: Vec<Option<String>>,
    b: Vec<Option<String>>,
    diff: Vec<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DataDetail {
    columns: Vec<String>,
    key_columns: Vec<usize>,
    key_mode: Option<KeyMode>,
    missing_in_a: Vec<usize>,
    ignored_columns: Vec<usize>,
    changed: Vec<ChangedRowView>,
    only_a: Vec<Vec<Option<String>>>,
    only_b: Vec<Vec<Option<String>>>,
    changed_total: usize,
    only_a_total: usize,
    only_b_total: usize,
    sample_limit: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableDetail {
    name: String,
    create_a: Option<String>,
    create_b: Option<String>,
    lines: Vec<DiffLine>,
    data: Option<DataDetail>,
}

const SAMPLE: usize = 300;

fn show_cell(c: &Cell, class: TypeClass) -> Option<String> {
    c.as_ref().map(|b| {
        if class.is_binary() || std::str::from_utf8(b).is_err() {
            let n = b.len().min(24);
            format!("0x{}{}", hex_encode(&b[..n]), if b.len() > n { "…" } else { "" })
        } else {
            let s = String::from_utf8_lossy(b);
            if s.chars().count() > 400 {
                format!("{}…", s.chars().take(400).collect::<String>())
            } else {
                s.into_owned()
            }
        }
    })
}

fn show_row(row: &[Cell], cls: &[TypeClass]) -> Vec<Option<String>> {
    row.iter().zip(cls).map(|(c, k)| show_cell(c, *k)).collect()
}

fn data_detail(d: &TableDataDiff, tb: &Table) -> DataDetail {
    let cls: Vec<TypeClass> =
        d.columns.iter().map(|n| tb.column(n).map(|c| type_class(&c.data_type)).unwrap_or(TypeClass::Other)).collect();
    DataDetail {
        columns: d.columns.clone(),
        key_columns: d.key_columns.clone(),
        key_mode: d.key_mode,
        missing_in_a: d.missing_in_a.clone(),
        ignored_columns: d.ignored_columns.clone(),
        changed: d
            .changed
            .iter()
            .take(SAMPLE)
            .map(|r| ChangedRowView { a: show_row(&r.a, &cls), b: show_row(&r.b, &cls), diff: changed_columns(d, r) })
            .collect(),
        only_a: d.only_in_a.iter().take(SAMPLE).map(|r| show_row(r, &cls)).collect(),
        only_b: d.only_in_b.iter().take(SAMPLE).map(|r| show_row(r, &cls)).collect(),
        changed_total: d.changed.len(),
        only_a_total: d.only_in_a.len(),
        only_b_total: d.only_in_b.len(),
        sample_limit: SAMPLE,
    }
}

fn side_by_side(a: &str, b: &str) -> Vec<DiffLine> {
    let la: Vec<&str> = a.lines().collect();
    let lb: Vec<&str> = b.lines().collect();
    let diff = TextDiff::from_slices(&la, &lb);
    let mut out = vec![];
    for op in diff.ops() {
        let (tag, ra, rb) = op.as_tag_tuple();
        match tag {
            DiffTag::Equal => {
                for (i, j) in ra.zip(rb) {
                    out.push(DiffLine { kind: "same", a: Some(la[i].into()), b: Some(lb[j].into()) });
                }
            }
            DiffTag::Delete => {
                for i in ra {
                    out.push(DiffLine { kind: "onlyA", a: Some(la[i].into()), b: None });
                }
            }
            DiffTag::Insert => {
                for j in rb {
                    out.push(DiffLine { kind: "onlyB", a: None, b: Some(lb[j].into()) });
                }
            }
            DiffTag::Replace => {
                let (va, vb): (Vec<usize>, Vec<usize>) = (ra.collect(), rb.collect());
                for k in 0..va.len().max(vb.len()) {
                    out.push(DiffLine {
                        kind: "changed",
                        a: va.get(k).map(|&i| la[i].to_string()),
                        b: vb.get(k).map(|&j| lb[j].to_string()),
                    });
                }
            }
        }
    }
    out
}

#[tauri::command]
async fn table_detail(state: State<'_, AppState>, name: String) -> Result<TableDetail, String> {
    let s = state.session.lock().unwrap().clone().ok_or("Belum ada hasil perbandingan")?;
    let r = &s.res;
    let t = r.schema_diff.iter().find(|t| t.name == name).ok_or("Tabel tidak ditemukan")?;
    let ta = t.name_a.as_deref().and_then(|n| r.schema_a.table(n));
    let tb = t.name_b.as_deref().and_then(|n| r.schema_b.table(n));
    let create_a = ta.map(|t| render_create_table(t, true, false));
    let create_b = tb.map(|t| render_create_table(t, true, false));
    let lines = side_by_side(create_a.as_deref().unwrap_or(""), create_b.as_deref().unwrap_or(""));
    let data = tb.and_then(|tb| r.data.iter().find(|d| d.table == tb.name).map(|d| data_detail(d, tb)));
    Ok(TableDetail { name, create_a, create_b, lines, data })
}

// ---------------------------------------------------------------------------
// Script

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScriptPreview {
    preview: String,
    truncated: bool,
    total_bytes: usize,
    statements: usize,
    warnings: Vec<String>,
}

const PREVIEW_BYTES: usize = 2 << 20;

fn utc_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[tauri::command]
async fn generate_script(state: State<'_, AppState>, options: ScriptOptions) -> Result<ScriptPreview, String> {
    let s = state.session.lock().unwrap().clone().ok_or("Belum ada hasil perbandingan")?;
    let script = off_runtime(move || {
        let r = &s.res;
        generate(
            &ScriptInput {
                schema_a: &r.schema_a,
                schema_b: &r.schema_b,
                data: &r.data,
                compare: &s.req.options,
                header: vec![
                    format!("Selisih - script sinkronisasi, dibuat {}", utc_now()),
                    format!("Target (A): {}", r.label_a),
                    format!("Sumber (B): {}", r.label_b),
                ],
            },
            &options,
        )
    })
    .await?;
    let total = script.sql.len();
    let mut cut = total.min(PREVIEW_BYTES);
    while !script.sql.is_char_boundary(cut) {
        cut -= 1;
    }
    let preview = ScriptPreview {
        preview: script.sql[..cut].to_string(),
        truncated: cut < total,
        total_bytes: total,
        statements: script.statements,
        warnings: script.warnings,
    };
    *state.script.lock().unwrap() = Some(Arc::new(script.sql));
    Ok(preview)
}

#[tauri::command]
async fn save_script(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let sql = state.script.lock().unwrap().clone().ok_or("Script belum dibuat")?;
    off_runtime(move || std::fs::write(&path, sql.as_bytes()).map_err(|e| e.to_string())).await?
}

#[tauri::command]
async fn execute_script(app: AppHandle, state: State<'_, AppState>, confirm: String) -> Result<ExecReport, String> {
    let s = state.session.lock().unwrap().clone().ok_or("Belum ada hasil perbandingan")?;
    let (params, db) = s.a_live.clone().ok_or("Sisi A bukan database live; simpan script lalu jalankan sendiri")?;
    if confirm != db {
        return Err("Nama database konfirmasi tidak cocok".into());
    }
    let sql = state.script.lock().unwrap().clone().ok_or("Script belum dibuat")?;
    state.cancel.store(false, Ordering::Relaxed);
    let cancel = state.cancel.clone();
    let rep = off_runtime(move || {
        selisih_core::live::execute_script(&params, &db, sql.as_bytes(), &Emit::new(app, cancel))
    })
    .await?
    .map_err(|e| e.to_string())?;
    Ok(rep)
}

// ---------------------------------------------------------------------------
// Koneksi

#[tauri::command]
fn list_connections(app: AppHandle) -> Result<Vec<ConnectionProfile>, String> {
    connections::load(&app)
}

#[tauri::command]
fn save_connection(app: AppHandle, profile: ConnectionProfile, password: Option<String>) -> Result<ConnectionProfile, String> {
    connections::upsert(&app, profile, password)
}

#[tauri::command]
fn delete_connection(app: AppHandle, id: String) -> Result<(), String> {
    connections::remove(&app, &id)
}

#[tauri::command]
async fn test_connection(profile: ConnectionProfile, password: Option<String>) -> Result<String, String> {
    let pw = password.filter(|p| !p.is_empty());
    let params = connections::params(&profile, pw);
    selisih_core::live::test_connection(&params).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_databases(app: AppHandle, id: String) -> Result<Vec<String>, String> {
    let prof = connections::find(&app, &id)?;
    let params = connections::params(&prof, None);
    selisih_core::live::list_databases(&params).await.map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            compare,
            cancel,
            table_detail,
            generate_script,
            save_script,
            execute_script,
            list_connections,
            save_connection,
            delete_connection,
            test_connection,
            list_databases,
        ])
        .run(tauri::generate_context!())
        .expect("gagal menjalankan Selisih");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JSON persis seperti yang dikirim UI (src/lib/types.ts).
    #[test]
    fn compare_input_matches_frontend_json() {
        let j = r#"{
          "a": {"kind": "file", "schemaPath": "/x/a.sql", "dataPath": null},
          "b": {"kind": "live", "connectionId": "c1", "database": "klinik"},
          "compareData": true,
          "options": {"ignoreIntDisplayWidth": true, "ignoreComments": false, "ignoreCharsetCollation": false,
                      "ignoreEngine": false, "ignoreRowFormat": true, "ignoreColumnOrder": true,
                      "ignoreTableNameCase": true, "ignoreAutoIncrement": true}
        }"#;
        let v: CompareInput = serde_json::from_str(j).unwrap();
        assert!(matches!(v.a, SideInput::File { ref schema_path, data_path: None } if schema_path == "/x/a.sql"));
        assert!(matches!(v.b, SideInput::Live { ref connection_id, ref database } if connection_id == "c1" && database == "klinik"));
        assert!(v.compare_data && v.options.ignore_column_order);
    }

    #[test]
    fn script_options_and_profile_from_frontend_json() {
        let o: ScriptOptions = serde_json::from_str(
            r#"{"includeSchema":true,"includeData":false,"dropTables":true,"dropColumns":true,"deleteRows":true,
                "disableFkChecks":false,"tables":["pasien"],"batchRows":250,"maxStatementBytes":1048576}"#,
        )
        .unwrap();
        assert!(!o.include_data && o.drop_tables && o.batch_rows == 250);
        assert_eq!(o.tables.as_deref(), Some(&["pasien".to_string()][..]));
        let p: ConnectionProfile = serde_json::from_str(
            r#"{"id":"","name":"Lokal","host":"127.0.0.1","port":3306,"user":"root","useTls":true,"acceptInvalidCerts":false,"hasPassword":true}"#,
        )
        .unwrap();
        assert!(p.use_tls && !p.has_password);
    }

    #[test]
    fn summary_serializes_with_frontend_names() {
        let v = serde_json::to_value(Status::OnlyInB).unwrap();
        assert_eq!(v, "onlyInB");
        let v = serde_json::to_value(KeyMode::FullRow).unwrap();
        assert_eq!(v, "fullRow");
        let line = serde_json::to_value(DiffLine { kind: "onlyA", a: Some("x".into()), b: None }).unwrap();
        assert_eq!(line["kind"], "onlyA");
        let rep = serde_json::to_value(ExecReport::default()).unwrap();
        assert!(rep.get("elapsedMs").is_some());
    }

    #[test]
    fn side_by_side_aligns_lines() {
        let rows = side_by_side("a\nb\nc", "a\nx\nc\nd");
        let kinds: Vec<&str> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, vec!["same", "changed", "same", "onlyB"]);
    }
}
