//! selisih: bandingkan dua sisi (file dump atau mysql://...) dari terminal.
//!
//! Contoh:
//!   selisih a.sql b.sql
//!   selisih a.schema.sql b.sql --a-data a.data.sql --script sync.sql
//!   selisih mysql://user:pass@127.0.0.1:3306/db_a b.sql --script sync.sql --apply
//!
//! Isi baris data tidak pernah dicetak, hanya hitungan.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use selisih_core::engine::{run_compare, CompareRequest, SideSpec};
use selisih_core::live::ConnParams;
use selisih_core::progress::Progress;
use selisih_core::schema_diff::Status;
use selisih_core::script::{generate, ScriptInput, ScriptOptions};

struct Stderr {
    quiet: bool,
}

impl Progress for Stderr {
    fn report(&self, phase: &str, detail: &str, done: u64, total: u64) {
        if self.quiet {
            return;
        }
        if total > 0 {
            eprint!("\r{phase} {detail} {:>5.1}%   ", done as f64 * 100.0 / total as f64);
        } else {
            eprint!("\r{phase} {detail} {done}        ");
        }
    }
}

fn parse_side(s: &str, data: Option<PathBuf>) -> Result<SideSpec, String> {
    if let Some(rest) = s.strip_prefix("mysql://") {
        // mysql://user[:pass]@host[:port]/db
        let (cred, hostdb) = rest.split_once('@').ok_or("URL butuh user@host")?;
        let (user, password) = cred.split_once(':').unwrap_or((cred, ""));
        let (hostport, db) = hostdb.split_once('/').ok_or("URL butuh /database")?;
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().map_err(|_| "port tidak valid")?),
            None => (hostport, 3306),
        };
        let password = std::env::var("SELISIH_PASSWORD").ok().filter(|_| password.is_empty()).unwrap_or(password.into());
        return Ok(SideSpec::Live {
            params: ConnParams { host: host.into(), port, user: user.into(), password, ..Default::default() },
            database: db.into(),
        });
    }
    Ok(SideSpec::File { schema_path: PathBuf::from(s), data_path: data })
}

fn usage() -> ExitCode {
    eprintln!(
        "pakai: selisih <A> <B> [--a-data FILE] [--b-data FILE] [--no-data] [--no-schema]\n\
         \x20              [--script OUT.sql] [--drop-tables] [--apply] [--quiet]\n\
         A/B: file .sql atau mysql://user:pass@host:port/db"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut pos = vec![];
    let (mut a_data, mut b_data, mut script_out) = (None, None, None);
    let mut req = CompareRequest::default();
    let mut sopts = ScriptOptions::default();
    let (mut apply, mut quiet) = (false, false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--a-data" => {
                i += 1;
                a_data = args.get(i).map(PathBuf::from);
            }
            "--b-data" => {
                i += 1;
                b_data = args.get(i).map(PathBuf::from);
            }
            "--script" => {
                i += 1;
                script_out = args.get(i).map(PathBuf::from);
            }
            "--no-data" => req.compare_data = false,
            "--no-schema" => sopts.include_schema = false,
            "--drop-tables" => sopts.drop_tables = true,
            "--apply" => apply = true,
            "--quiet" => quiet = true,
            "-h" | "--help" => return usage(),
            s => pos.push(s.to_string()),
        }
        i += 1;
    }
    if pos.len() != 2 {
        return usage();
    }
    req.a = match parse_side(&pos[0], a_data) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("A: {e}");
            return ExitCode::from(2);
        }
    };
    req.b = match parse_side(&pos[1], b_data) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("B: {e}");
            return ExitCode::from(2);
        }
    };
    sopts.include_data = req.compare_data;
    let prog = Stderr { quiet };
    let t0 = Instant::now();
    let res = match run_compare(&req, &prog) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("\ngagal: {e}");
            return ExitCode::from(1);
        }
    };
    if !quiet {
        eprintln!();
    }
    println!("A: {} ({} tabel)", res.label_a, res.schema_a.tables.len());
    println!("B: {} ({} tabel)", res.label_b, res.schema_b.tables.len());
    let mut diffs = 0;
    for t in &res.schema_diff {
        match t.status {
            Status::Same => {}
            Status::OnlyInA => {
                diffs += 1;
                println!("  - {} (hanya di A)", t.name)
            }
            Status::OnlyInB => {
                diffs += 1;
                println!("  + {} (hanya di B)", t.name)
            }
            Status::Modified => {
                diffs += 1;
                println!("  ~ {}", t.name);
                for c in &t.changes {
                    println!("      {} {} {}: {}", c.action, c.category, c.name, c.detail);
                }
            }
        }
    }
    if let Some(r) = &res.data_skipped {
        println!("data: dilewati: {r}");
    }
    for d in &res.data {
        if !d.is_same() {
            diffs += 1;
            println!(
                "  data {}: sama {}, berubah {}, hanya A {}, hanya B {}",
                d.table,
                d.same,
                d.changed.len(),
                d.only_in_a.len(),
                d.only_in_b.len()
            );
        }
    }
    for (t, e) in &res.data_errors {
        println!("  data {t}: ERROR {e}");
    }
    for w in &res.warnings {
        println!("  peringatan: {w}");
    }
    println!("selesai dalam {} ms, {} perbedaan", t0.elapsed().as_millis(), diffs);

    if script_out.is_some() || apply {
        let script = generate(
            &ScriptInput {
                schema_a: &res.schema_a,
                schema_b: &res.schema_b,
                data: &res.data,
                compare: &req.options,
                header: vec!["Selisih: script sinkronisasi A <- B".to_string(), format!("A: {}", res.label_a), format!("B: {}", res.label_b)],
            },
            &sopts,
        );
        for w in &script.warnings {
            println!("  script: {w}");
        }
        if let Some(p) = &script_out {
            if let Err(e) = std::fs::write(p, &script.sql) {
                eprintln!("gagal tulis script: {e}");
                return ExitCode::from(1);
            }
            println!("script: {} statement -> {}", script.statements, p.display());
        }
        if apply {
            let SideSpec::Live { params, database } = &req.a else {
                eprintln!("--apply hanya untuk A berupa mysql://");
                return ExitCode::from(2);
            };
            match selisih_core::live::execute_script(params, database, script.sql.as_bytes(), &prog) {
                Ok(rep) => {
                    if !quiet {
                        eprintln!();
                    }
                    match rep.error {
                        None => println!("apply: {} statement OK ({} ms)", rep.executed, rep.elapsed_ms),
                        Some(e) => {
                            println!("apply GAGAL di statement #{}: {}\n{}", e.index + 1, e.message, e.statement);
                            return ExitCode::from(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("apply gagal: {e}");
                    return ExitCode::from(1);
                }
            }
        }
    }
    ExitCode::from(if diffs > 0 { 10 } else { 0 })
}
