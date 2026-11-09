//! Sumber dari database live (MySQL/MariaDB).
//!
//! Schema diambil lewat `SHOW CREATE TABLE` dan di-parse dengan parser yang
//! sama dengan file dump (mysqldump juga memakai SHOW CREATE TABLE), jadi
//! file dan live menghasilkan model yang identik. Data dibaca lewat
//! protokol teks supaya format nilai sama dengan isi dump.

use mysql_async::prelude::*;
use mysql_async::{Conn, Opts, OptsBuilder, SslOpts, Value};
use serde::{Deserialize, Serialize};

use crate::ddl::{classify, Stmt};
use crate::lexer::StatementReader;
use crate::model::{Schema, SkippedObject, Table, TypeClass, type_class};
use crate::normalize::canonicalize_schema;
use crate::progress::Progress;
use crate::render::q;
use crate::row::*;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnParams {
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default, skip_serializing)]
    pub password: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub use_tls: bool,
    #[serde(default)]
    pub accept_invalid_certs: bool,
}

fn db_err(e: mysql_async::Error) -> Error {
    Error::Db(e.to_string())
}

fn opts(p: &ConnParams) -> Opts {
    let mut b = OptsBuilder::default()
        .ip_or_hostname(p.host.clone())
        .tcp_port(if p.port == 0 { 3306 } else { p.port })
        .user(Some(p.user.clone()))
        .pass(Some(p.password.clone()))
        .db_name(p.database.clone().filter(|d| !d.is_empty()))
        .prefer_socket(false)
        .stmt_cache_size(0)
        .init(vec!["SET NAMES utf8mb4", "SET time_zone = '+00:00'"]);
    if p.use_tls {
        b = b.ssl_opts(SslOpts::default().with_danger_accept_invalid_certs(p.accept_invalid_certs));
    }
    b.into()
}

pub async fn connect(p: &ConnParams) -> Result<Conn> {
    Conn::new(opts(p)).await.map_err(db_err)
}

pub async fn server_version(conn: &mut Conn) -> Result<String> {
    let v: Option<String> = conn.query_first("SELECT VERSION()").await.map_err(db_err)?;
    Ok(v.unwrap_or_default())
}

/// Tes koneksi; kembalikan versi server.
pub async fn test_connection(p: &ConnParams) -> Result<String> {
    let mut c = connect(p).await?;
    let v = server_version(&mut c).await?;
    let _ = c.disconnect().await;
    Ok(v)
}

pub async fn list_databases(p: &ConnParams) -> Result<Vec<String>> {
    let mut c = connect(p).await?;
    let v: Vec<String> = c.query("SHOW DATABASES").await.map_err(db_err)?;
    let _ = c.disconnect().await;
    Ok(v
        .into_iter()
        .filter(|d| !["information_schema", "performance_schema", "mysql", "sys"].contains(&d.as_str()))
        .collect())
}

async fn load_schema_async(conn: &mut Conn, db: &str, progress: &dyn Progress) -> Result<Schema> {
    let tables: Vec<(String, String)> = conn
        .exec(
            "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (db,),
        )
        .await
        .map_err(db_err)?;
    let mut s = Schema::default();
    let total = tables.len() as u64;
    for (i, (name, ttype)) in tables.iter().enumerate() {
        progress.check()?;
        if i % 20 == 0 {
            progress.report("Membaca schema live", db, i as u64, total);
        }
        if ttype == "VIEW" {
            s.skipped.push(SkippedObject { kind: "view".into(), name: name.clone() });
            continue;
        }
        if ttype == "SEQUENCE" {
            s.skipped.push(SkippedObject { kind: "sequence".into(), name: name.clone() });
            continue;
        }
        let row: Option<mysql_async::Row> = conn
            .query_first(format!("SHOW CREATE TABLE {}.{}", q(db), q(name)))
            .await
            .map_err(db_err)?;
        let Some(row) = row else { continue };
        let ddl: Vec<u8> = match row.as_ref(1) {
            Some(Value::Bytes(b)) => b.clone(),
            _ => continue,
        };
        match classify(&ddl) {
            Ok(Stmt::CreateTable(t)) => s.upsert(t),
            Ok(_) => {}
            Err(e) => s.warnings.push(format!("tabel `{name}`: {e}")),
        }
    }
    for (kind, sql) in [
        ("trigger", "SELECT TRIGGER_NAME FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = ?"),
        ("routine", "SELECT ROUTINE_NAME FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA = ?"),
        ("event", "SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA = ?"),
    ] {
        if let Ok(names) = conn.exec::<String, _, _>(sql, (db,)).await {
            for name in names {
                s.skipped.push(SkippedObject { kind: kind.into(), name });
            }
        }
    }
    canonicalize_schema(&mut s);
    progress.report("Membaca schema live", db, total, total);
    Ok(s)
}

/// Koneksi sinkron (runtime sendiri) untuk dipakai dari thread worker.
pub struct LiveSource {
    rt: tokio::runtime::Runtime,
    conn: Conn,
    db: String,
    row: RowBuf,
    pub version: String,
}

impl LiveSource {
    pub fn connect(p: &ConnParams, db: &str) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let mut params = p.clone();
        params.database = Some(db.to_string());
        let (conn, version) = rt.block_on(async {
            let mut c = connect(&params).await?;
            let v = server_version(&mut c).await?;
            Ok::<_, Error>((c, v))
        })?;
        Ok(LiveSource { rt, conn, db: db.to_string(), row: RowBuf::default(), version })
    }

    pub fn load_schema(&mut self, progress: &dyn Progress) -> Result<Schema> {
        let (conn, db) = (&mut self.conn, &self.db);
        self.rt.block_on(load_schema_async(conn, db, progress))
    }
}

impl RowSource for LiveSource {
    fn has_data(&self) -> bool {
        true
    }

    fn for_each_row(&mut self, table: &Table, f: &mut RowFn<'_>) -> Result<()> {
        let cols: Vec<String> = table
            .columns
            .iter()
            .map(|c| {
                if type_class(&c.data_type) == TypeClass::Bit {
                    format!("CAST({} AS UNSIGNED)", q(&c.name))
                } else {
                    q(&c.name)
                }
            })
            .collect();
        let sql = format!("SELECT {} FROM {}.{}", cols.join(", "), q(&self.db), q(&table.name));
        let map: Vec<Option<usize>> = (0..table.columns.len()).map(Some).collect();
        let (conn, buf) = (&mut self.conn, &mut self.row);
        self.rt.block_on(async {
            let mut res = conn.query_iter(sql).await.map_err(db_err)?;
            while let Some(row) = res.next().await.map_err(db_err)? {
                buf.clear();
                for i in 0..row.len() {
                    match row.as_ref(i) {
                        Some(Value::Bytes(b)) => buf.push(b, KIND_STR),
                        Some(Value::NULL) | None => buf.push_null(),
                        Some(other) => {
                            let txt = other.as_sql(true);
                            buf.push(txt.trim_matches('\'').as_bytes(), KIND_NUM)
                        }
                    }
                }
                f(&RowView { buf, map: &map })?;
            }
            Ok(())
        })
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecReport {
    pub total: usize,
    pub executed: usize,
    pub error: Option<ExecError>,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecError {
    pub index: usize,
    pub statement: String,
    pub message: String,
    /// True bila transaksi DML terakhir di-rollback.
    pub rolled_back: bool,
}

fn is_dml(stmt: &[u8]) -> bool {
    let t = crate::lexer::trim_start(stmt);
    ["INSERT", "UPDATE", "DELETE", "REPLACE"]
        .iter()
        .any(|k| t.len() >= k.len() && t[..k.len()].eq_ignore_ascii_case(k.as_bytes()))
}

/// Jalankan script statement demi statement. DML dikelompokkan dalam
/// transaksi; DDL di luar transaksi (commit implisit MySQL).
/// Berhenti di error pertama.
pub fn execute_script(p: &ConnParams, db: &str, sql: &[u8], progress: &dyn Progress) -> Result<ExecReport> {
    let mut stmts = vec![];
    let mut rd = StatementReader::new(sql);
    while let Some(s) = rd.next_statement()? {
        stmts.push(s.bytes);
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let mut params = p.clone();
    params.database = Some(db.to_string());
    let start = std::time::Instant::now();
    rt.block_on(async {
        let mut conn = connect(&params).await?;
        let mut rep = ExecReport { total: stmts.len(), ..Default::default() };
        let mut in_tx = false;
        for (i, s) in stmts.iter().enumerate() {
            if progress.is_cancelled() {
                if in_tx {
                    let _ = conn.query_drop("ROLLBACK").await;
                }
                return Err(Error::Cancelled);
            }
            progress.report("Menjalankan script", "", i as u64, stmts.len() as u64);
            let dml = is_dml(s);
            if dml && !in_tx {
                conn.query_drop("START TRANSACTION").await.map_err(db_err)?;
                in_tx = true;
            } else if !dml && in_tx {
                conn.query_drop("COMMIT").await.map_err(db_err)?;
                in_tx = false;
            }
            // Kirim byte apa adanya: dump bisa berisi string biner mentah.
            if let Err(e) = conn.query_drop(&s[..]).await {
                let text = String::from_utf8_lossy(s);
                let mut rolled_back = false;
                if in_tx {
                    rolled_back = conn.query_drop("ROLLBACK").await.is_ok();
                }
                let preview: String = text.chars().take(300).collect();
                rep.error = Some(ExecError { index: i, statement: preview, message: e.to_string(), rolled_back });
                rep.elapsed_ms = start.elapsed().as_millis();
                let _ = conn.disconnect().await;
                return Ok(rep);
            }
            rep.executed += 1;
        }
        if in_tx {
            conn.query_drop("COMMIT").await.map_err(db_err)?;
        }
        let _ = conn.disconnect().await;
        rep.elapsed_ms = start.elapsed().as_millis();
        progress.report("Menjalankan script", "", stmts.len() as u64, stmts.len() as u64);
        Ok(rep)
    })
}
