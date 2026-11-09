# Selisih

Aplikasi desktop macOS (Apple Silicon) untuk membandingkan dan menyinkronkan schema + data MySQL/MariaDB. Sumber bisa file dump (`mysqldump` / `mariadb-dump`) atau database live.

- **A** = target, yang akan diubah oleh script.
- **B** = acuan, kondisi yang diinginkan.

Penjelasan lengkap proyek (tujuan, alur kerja, cara kerja engine, status terhadap rencana): [docs/tentang-proyek.md](docs/tentang-proyek.md).

## Fitur

- Schema diff: tabel, kolom, tipe, default, null, auto increment, PK, index, FK, CHECK, engine, charset/collation, komentar, urutan kolom.
- Data diff berdasarkan primary key (atau unique NOT NULL, atau baris utuh untuk tabel tanpa key).
- Dump schema dan data boleh dari file terpisah (mis. `tes_0.sql` + `tes.sql`).
- Normalisasi MySQL 8 vs MariaDB (lihat [docs/spec-m0.md](docs/spec-m0.md)).
- Script sinkronisasi yang aman terhadap FK; preview, simpan `.sql`, atau jalankan di A setelah konfirmasi.
- Profil koneksi di config app; password hanya di Keychain macOS.
- CLI `selisih` untuk otomasi dan benchmark.

## Menjalankan

Butuh Rust (stable) dan Node.js.

```bash
npm install
```

```bash
npm run tauri dev
```

Build `.app` dan `.dmg` (arm64):

```bash
npm run tauri build
```

Hasil ada di `target/release/bundle/`. Bila langkah DMG gagal dengan "Finder got an error: AppleEvent timed out" (terminal tanpa izin Automation), jalankan dengan `CI=true` supaya styling jendela DMG dilewati. Build ini belum di-sign. Untuk distribusi tanpa peringatan Gatekeeper, perlu Apple Developer ID: set `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`, lalu build ulang (Tauri melakukan sign + notarize).

UI saja di browser, dengan backend tiruan:

```bash
npm run dev
```

## CLI

```bash
cargo run --release -p selisih-cli -- fixtures/klinik_a.sql fixtures/klinik_b.sql --script sync.sql
```

```bash
cargo run --release -p selisih-cli -- schema_a.sql b.sql --a-data data_a.sql --no-data
```

Sisi live: `mysql://user@host:port/db` (password lewat `SELISIH_PASSWORD`). `--apply` menjalankan script ke A live. Exit code 0 = sama, 10 = ada perbedaan. Isi baris tidak pernah dicetak.

## Test

```bash
cargo test --workspace
```

Round-trip ke server sungguhan (buat database `selisih_rt_*`, terapkan script, bandingkan ulang harus kosong):

```bash
docker compose up -d mariadb11
```

```bash
SELISIH_TEST_DB=mysql://root:selisih_test@127.0.0.1:33114 cargo test -p selisih-core
```

Fixture di `fixtures/` sintetis (data klinik karangan): `klinik_a.sql` dan `klinik_b.sql` dibuat dengan `mariadb-dump` dari `fixtures/src/*.sql`; `klinik_a.mysql8.schema.sql` adalah schema yang sama dalam format MySQL 8.

## Struktur

```
crates/core      engine (tanpa UI)
  lexer          splitter statement streaming + tokenizer
  ddl            CREATE TABLE -> model
  normalize      aturan MySQL vs MariaDB
  dump           scan file, indeks offset INSERT, baca baris per tabel
  live           SHOW CREATE TABLE + SELECT protokol teks, eksekusi script
  schema_diff    perbandingan objek
  data_diff      hash per primary key (xxh3)
  script         ALTER/DML dengan urutan FK
  engine         orkestrasi dua sisi
crates/cli       CLI `selisih`
src-tauri        shell Tauri 2: command, Keychain, event progress
src              UI React + TypeScript
```

## Keputusan yang berbeda dari rencana awal

1. **Parser sendiri, bukan crate `sqlparser`.** Output mysqldump sangat teratur, sedangkan `sqlparser` tidak mengerti `/*!..*/`, `DELIMITER`, opsi tabel MariaDB, dan terlalu lambat untuk extended insert 1 MB. Parser sendiri streaming dan membaca byte (aman untuk data biner).
2. **Live memakai `SHOW CREATE TABLE`, bukan menyusun dari `information_schema`.** mysqldump juga memakai `SHOW CREATE TABLE`, jadi file dan live melewati parser yang sama dan hasilnya identik. `information_schema` hanya dipakai untuk daftar tabel, trigger, routine, event.
3. **`mysql_async`** dengan protokol teks, supaya nilai data sama persis dengan isi dump.

## Performa (Mac Apple Silicon pengembang, build release, dump sintetis)

| Kasus | Waktu | RAM puncak |
|---|---|---|
| 2 dump × 1 GB, ±17 juta baris per sisi, perubahan kecil | 17,5 dtk | 1,1 GB |
| Sama, tetapi hampir semua baris berubah (kasus terburuk) | 25 dtk + 16 dtk tulis script | 5,7 GB |

RAM didominasi peta hash tabel terbesar (±32 byte per baris) dan baris yang berbeda (disimpan penuh untuk script).

## Batasan yang diketahui

- Baris yang berbeda disimpan penuh di memori. Dua sisi yang sangat berbeda butuh RAM besar. Rencana M8: tulis script langsung ke file (streaming) dan spill ke disk.
- Data live dibaca utuh lewat `SELECT`; belum ada hash per chunk di sisi server.
- Rename kolom/tabel terbaca sebagai drop + add.
- View, trigger, routine, event, partisi: dideteksi tetapi tidak dibandingkan.
- Collation-aware key (mis. `'abc'` vs `'ABC'` pada `_ci`) dibandingkan per byte; hasilnya tetap benar (delete + insert), hanya kurang ringkas.
- Belum diuji ke server MySQL 8 sungguhan (hanya MariaDB 13 lokal + fixture format MySQL 8). Gunakan `docker-compose.yml`.
