//! Golden test + property test di atas fixture sintetis `fixtures/`.
//! Round-trip ke DB live hanya jalan bila `SELISIH_TEST_DB` di-set, contoh:
//!   SELISIH_TEST_DB=mysql://selisih:selisih_test@127.0.0.1:3399 cargo test

use std::path::PathBuf;

use selisih_core::engine::{run_compare, CompareRequest, CompareResult, SideSpec};
use selisih_core::progress::NoProgress;
use selisih_core::schema_diff::Status;
use selisih_core::script::{generate, ScriptInput, ScriptOptions};

fn fx(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

fn file(schema: &str, data: Option<&str>) -> SideSpec {
    SideSpec::File { schema_path: fx(schema), data_path: data.map(fx) }
}

fn compare(a: SideSpec, b: SideSpec) -> (CompareRequest, CompareResult) {
    let req = CompareRequest { a, b, ..Default::default() };
    let res = run_compare(&req, &NoProgress).expect("compare");
    (req, res)
}

fn diff_count(r: &CompareResult) -> usize {
    r.schema_diff.iter().filter(|t| t.status != Status::Same).count() + r.data.iter().filter(|d| !d.is_same()).count()
}

fn script(req: &CompareRequest, res: &CompareResult, opts: &ScriptOptions) -> String {
    generate(
        &ScriptInput { schema_a: &res.schema_a, schema_b: &res.schema_b, data: &res.data, compare: &req.options, header: vec![] },
        opts,
    )
    .sql
}

#[test]
fn identical_inputs_have_no_diff() {
    let (_, r) = compare(file("klinik_a.sql", None), file("klinik_a.sql", None));
    assert_eq!(diff_count(&r), 0);
    assert!(r.data.iter().all(|d| d.rows_a == d.rows_b && d.same == d.rows_a));
}

#[test]
fn mysql8_and_mariadb_dumps_of_same_schema_are_equal() {
    let (_, r) = compare(file("klinik_a.mysql8.schema.sql", None), file("klinik_a.schema.sql", None));
    let diffs: Vec<_> = r.schema_diff.iter().filter(|t| t.status != Status::Same).collect();
    assert!(diffs.is_empty(), "{diffs:#?}");
    assert!(r.data_skipped.is_some());
    assert!(r.schema_a.skipped.iter().any(|o| o.kind == "view" && o.name == "v_dokter_aktif"));
}

#[test]
fn split_schema_and_data_equals_single_dump() {
    let (_, r) = compare(file("klinik_a.schema.sql", Some("klinik_a.data.sql")), file("klinik_a.sql", None));
    assert_eq!(diff_count(&r), 0);
    assert_eq!(r.data.iter().map(|d| d.rows_a).sum::<u64>(), 27);
}

#[test]
fn golden_diff_a_to_b() {
    let (req, r) = compare(file("klinik_a.sql", None), file("klinik_b.sql", None));
    let st = |n: &str| r.schema_diff.iter().find(|t| t.name == n).unwrap();
    assert_eq!(st("tarif").status, Status::OnlyInB);
    assert_eq!(st("tarif_lama").status, Status::OnlyInA);
    assert_eq!(st("resep_item").status, Status::Same);
    assert_eq!(st("log_akses").status, Status::Same);
    let pasien: Vec<(&str, &str, &str)> =
        st("pasien").changes.iter().map(|c| (c.category, c.action, c.name.as_str())).collect();
    for want in [
        ("column", "removed", "telepon"),
        ("column", "added", "email"),
        ("index", "modified", "idx_nama"),
        ("index", "added", "idx_tgl"),
    ] {
        assert!(pasien.contains(&want), "pasien tidak punya {want:?}: {pasien:?}");
    }
    assert!(st("dokter").changes.iter().any(|c| c.category == "foreign_key" && c.action == "modified"));
    assert!(st("kunjungan").changes.iter().any(|c| c.category == "option" && c.name == "COMMENT"));

    let d = |n: &str| r.data.iter().find(|d| d.table == n).unwrap();
    let counts = |n: &str| {
        let x = d(n);
        (x.same, x.changed.len(), x.only_in_a.len(), x.only_in_b.len())
    };
    assert_eq!(counts("pasien"), (1, 3, 1, 1));
    assert_eq!(counts("poli"), (1, 2, 0, 1));
    assert_eq!(counts("kunjungan"), (3, 1, 1, 1)); // kolom baru `status` dibandingkan via default
    assert_eq!(counts("log_akses"), (3, 0, 1, 1)); // tanpa PK: multiset
    assert_eq!(counts("tarif"), (0, 0, 0, 3));

    let sql = script(&req, &r, &ScriptOptions::default());
    let pos = |needle: &str| sql.find(needle).unwrap_or_else(|| panic!("tidak ada `{needle}` di script:\n{sql}"));
    // Anak dihapus sebelum induk, induk diisi sebelum anak, FK di akhir.
    assert!(pos("DELETE FROM `kunjungan`") < pos("DELETE FROM `pasien`"));
    assert!(pos("DELETE FROM `kunjungan`") < pos("DELETE FROM `dokter`"));
    assert!(pos("INSERT INTO `poli`") < pos("INSERT INTO `dokter`"));
    assert!(pos("INSERT INTO `dokter`") < pos("INSERT INTO `kunjungan`"));
    assert!(pos("INSERT INTO `tarif`") < pos("ADD CONSTRAINT `fk_tarif_poli`"));
    assert!(pos("DROP FOREIGN KEY `fk_dokter_poli`") < pos("MODIFY COLUMN `sip`"));
    assert!(!sql.contains("DROP TABLE"), "drop tabel harus opt-in");
    let with_drop = script(&req, &r, &ScriptOptions { drop_tables: true, ..Default::default() });
    assert!(with_drop.contains("DROP TABLE `tarif_lama`"));
    // Nilai biner dan BIT ditulis dengan aman.
    assert!(sql.contains("`foto`=0x89504E470D0A1A0A00"));
    assert!(sql.contains("`flags`=10"));
}

#[test]
fn dump_reader_offsets_cover_every_insert() {
    let idx = selisih_core::dump::scan_dump(&fx("klinik_b.sql"), &NoProgress, "B").unwrap();
    assert_eq!(idx.insert_statements, 7);
    let raw = std::fs::read(fx("klinik_b.sql")).unwrap();
    for ranges in idx.inserts.values() {
        for &(off, len) in ranges {
            let s = &raw[off as usize..(off + len) as usize];
            assert!(s.starts_with(b"INSERT INTO"), "{}", String::from_utf8_lossy(&s[..20]));
        }
    }
}

// ---------------------------------------------------------------------------
// Round-trip ke DB live (opsional)

#[cfg(feature = "live")]
mod live {
    use super::*;
    use selisih_core::live::{execute_script, ConnParams};

    fn params() -> Option<ConnParams> {
        let url = std::env::var("SELISIH_TEST_DB").ok()?;
        let rest = url.strip_prefix("mysql://")?;
        let (cred, host) = rest.split_once('@')?;
        let (user, password) = cred.split_once(':').unwrap_or((cred, ""));
        let host = host.trim_end_matches('/');
        let (host, port) = host.rsplit_once(':').map(|(h, p)| (h, p.parse().unwrap())).unwrap_or((host, 3306));
        Some(ConnParams { host: host.into(), port, user: user.into(), password: password.into(), ..Default::default() })
    }

    fn reset_db(p: &ConnParams, db: &str, src: &str) {
        let sql = format!("DROP DATABASE IF EXISTS `{db}`; CREATE DATABASE `{db}`;");
        let r = execute_script(p, "", sql.as_bytes(), &NoProgress).unwrap();
        assert!(r.error.is_none(), "{:?}", r.error);
        let body = std::fs::read(fx(src)).unwrap();
        let r = execute_script(p, db, &body, &NoProgress).unwrap();
        assert!(r.error.is_none(), "{:?}", r.error);
    }

    fn live(p: &ConnParams, db: &str) -> SideSpec {
        SideSpec::Live { params: p.clone(), database: db.into() }
    }

    #[test]
    fn round_trip_file_and_live() {
        let Some(p) = params() else {
            eprintln!("SELISIH_TEST_DB tidak di-set, round-trip dilewati");
            return;
        };
        reset_db(&p, "selisih_rt_a", "klinik_a.sql");
        reset_db(&p, "selisih_rt_b", "klinik_b.sql");

        // live vs live sama dengan file vs file
        let (_, files) = compare(file("klinik_a.sql", None), file("klinik_b.sql", None));
        let (req, lives) = compare(live(&p, "selisih_rt_a"), live(&p, "selisih_rt_b"));
        assert_eq!(diff_count(&files), diff_count(&lives));

        // terapkan script A <- B lalu bandingkan lagi: harus kosong
        let sql = script(&req, &lives, &ScriptOptions { drop_tables: true, ..Default::default() });
        let rep = execute_script(&p, "selisih_rt_a", sql.as_bytes(), &NoProgress).unwrap();
        assert!(rep.error.is_none(), "{:?}", rep.error);
        let (_, after) = compare(live(&p, "selisih_rt_a"), live(&p, "selisih_rt_b"));
        assert_eq!(diff_count(&after), 0, "{:#?}", after.schema_diff);
        let (_, vs_file) = compare(live(&p, "selisih_rt_a"), file("klinik_b.sql", None));
        assert_eq!(diff_count(&vs_file), 0);

        // dan sebaliknya: B <- A
        reset_db(&p, "selisih_rt_b", "klinik_b.sql");
        let (req, r) = compare(live(&p, "selisih_rt_b"), file("klinik_a.sql", None));
        let sql = script(&req, &r, &ScriptOptions { drop_tables: true, ..Default::default() });
        let rep = execute_script(&p, "selisih_rt_b", sql.as_bytes(), &NoProgress).unwrap();
        assert!(rep.error.is_none(), "{:?}\n{sql}", rep.error);
        let (_, after) = compare(live(&p, "selisih_rt_b"), file("klinik_a.sql", None));
        assert_eq!(diff_count(&after), 0, "{:#?}", after.schema_diff);

        let _ = execute_script(&p, "", b"DROP DATABASE `selisih_rt_a`; DROP DATABASE `selisih_rt_b`;", &NoProgress);
    }
}
