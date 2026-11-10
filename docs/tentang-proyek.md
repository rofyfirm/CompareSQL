# Tentang Selisih

Selisih adalah aplikasi desktop macOS untuk menjawab dua pertanyaan tentang dua database MySQL atau MariaDB:

1. **Apa bedanya?** Struktur tabel (schema) dan isi baris (data).
2. **Bagaimana menyamakannya?** Script SQL yang, bila dijalankan di satu sisi, membuatnya sama dengan sisi lain.

Fungsinya mirip fitur "structure/data synchronization" di Navicat atau SQLyog, tetapi fokus hanya pada perbandingan dan sinkronisasi. Selisih tidak meniru kode, tampilan, atau nama produk tersebut.

## Kenapa dibuat

Kasus yang memicu proyek ini: database operasional (contoh: sistem klinik) dengan ratusan sampai ribuan tabel, dump berukuran lebih dari 1 GB, dan server campuran MySQL 8 dan MariaDB. Masalah yang sering muncul:

- Dump dari MySQL dan MariaDB menulis hal yang sama dengan cara berbeda (`int(11)` vs `int`, `current_timestamp()` vs `CURRENT_TIMESTAMP`, JSON sebagai `longtext`). Diff teks biasa penuh "beda palsu".
- Mengubah struktur tabel yang saling terhubung foreign key harus dengan urutan yang benar, kalau tidak `ALTER` gagal di tengah jalan.
- Dump besar tidak bisa dibuka di editor, apalagi dibandingkan baris per baris secara manual.

## Konsep dasar: sisi A dan sisi B

| Sisi | Peran | Contoh |
|---|---|---|
| **A** | Target. Yang akan diubah oleh script. | database staging, salinan lokal |
| **B** | Acuan. Kondisi yang diinginkan. | dump produksi terbaru, schema versi baru |

Tiap sisi bisa berupa:

- **File dump** dari `mysqldump` / `mariadb-dump`. Schema dan data boleh di file terpisah, misalnya satu file `--no-data` dan satu file `--no-create-info`.
- **Database live** lewat koneksi MySQL/MariaDB.

Tombol ⇄ menukar A dan B bila arah sinkronisasi ingin dibalik.

## Alur kerja pengguna

1. Pilih sumber A dan B (file atau koneksi). Profil koneksi disimpan; password masuk Keychain macOS.
2. Klik **Bandingkan** (⌘↩). Dialog progres menampilkan tahap dan bisa dibatalkan.
3. Telusuri hasil di panel kiri. Filter: berbeda, hanya di A, hanya di B, sama. Tiap tabel menampilkan jumlah perubahan schema dan jumlah baris berubah/tambah/hapus.
4. Buka satu tabel:
   - Tab **Schema**: daftar perubahan (kolom, index, FK, CHECK, opsi) dan definisi `CREATE TABLE` A dan B berdampingan dengan baris yang berbeda diwarnai.
   - Tab **Data**: ringkasan jumlah baris, lalu tabel baris yang berubah (sel yang beda disorot), baris yang hanya ada di A, dan yang hanya ada di B.
5. Centang tabel yang ingin disinkronkan, buka **Script sinkron**, atur opsi (ikutkan schema/data, izinkan drop tabel/kolom, hapus baris).
6. Tinjau script, lalu **Simpan .sql** atau **Jalankan di A**. Eksekusi hanya tersedia bila A adalah database live, dan harus dikonfirmasi dengan mengetik nama database.
7. Setelah eksekusi, **Bandingkan ulang** untuk memastikan A sudah sama dengan B.

## Cara kerja di balik layar

```
 File dump ─┐                                           ┌─ Schema diff ─┐
            ├─► Model schema ─► Normalisasi ─► Pasangkan ┤               ├─► Script sinkron ─► Simpan / Jalankan
 DB live  ──┘   + sumber baris                  tabel    └─ Data diff  ──┘
```

### 1. Membaca sumber

- **File dump** dibaca sekali secara streaming. Statement dipotong pada `;` yang berada di luar string dan komentar, termasuk perintah `DELIMITER` dan komentar bersyarat `/*!40101 ... */`. `CREATE TABLE` di-parse menjadi model. Untuk `INSERT`, yang dicatat hanya posisi byte di file; isinya baru dibaca saat data diff, tabel demi tabel. Dengan cara ini dump 1 GB tidak perlu dimuat utuh ke memori.
- **Database live** dibaca dengan `SHOW CREATE TABLE`, perintah yang sama yang dipakai mysqldump. Hasilnya lewat parser yang sama, jadi file dan live menghasilkan model yang identik. Data dibaca lewat protokol teks dengan zona waktu UTC, supaya nilainya persis sama dengan isi dump.

### 2. Normalisasi

Sebelum dibandingkan, kedua model dirapikan supaya perbedaan tampilan tidak dianggap perbedaan isi. Contoh: `utf8` = `utf8mb3`, `DEFAULT '0'` = `DEFAULT 0` pada kolom angka, `DEFAULT NULL` pada kolom nullable = tanpa default, `int(11)` = `int` (bisa dimatikan). Aturan lengkapnya ada di [spec-m0.md](spec-m0.md).

### 3. Schema diff

Tabel dipasangkan berdasarkan nama. Di dalam tabel, kolom dan index dicocokkan per nama; foreign key per nama lalu per definisi (nama otomatis seperti `t_ibfk_1` bisa beda antar server); CHECK per ekspresi. Kolom yang pindah posisi dideteksi dengan *longest common subsequence* urutan kolom.

### 4. Data diff

Untuk tiap tabel:

1. Pilih kunci: primary key, atau unique index yang semua kolomnya `NOT NULL`. Tabel tanpa kunci dibandingkan sebagai kumpulan baris utuh.
2. Baca semua baris A. Simpan hash kunci (128 bit) dan hash isi baris (64 bit), sekitar 32 byte per baris.
3. Baca semua baris B. Untuk tiap baris: kunci tidak ada di A → **hanya di B**; hash isi beda → **berubah**; sama → dihitung saja.
4. Kunci A yang tidak pernah muncul di B → **hanya di A**. Baris A yang perlu ditampilkan atau dipakai script diambil dengan membaca A sekali lagi.

Kolom yang baru ada di B dibandingkan memakai nilai default B, karena itulah nilai yang akan diisi MySQL setelah `ADD COLUMN`.

### 5. Script sinkron

Urutan script dirancang supaya foreign key tidak menggagalkan proses:

1. Hapus foreign key yang berubah atau yang menyentuh kolom/index yang akan diubah.
2. Drop tabel yang hanya ada di A (opsional, mati secara default).
3. Buat tabel baru tanpa foreign key.
4. `ALTER TABLE`: drop index dan kolom, tambah/ubah kolom sesuai urutan B, tambah index.
5. Hapus baris, tabel anak dulu.
6. Ubah dan tambah baris, tabel induk dulu.
7. Pasang kembali semua foreign key.

Urutan induk-anak dihitung dengan *topological sort* atas relasi foreign key. Nilai biner ditulis sebagai `0x...`, string di-escape gaya MySQL, dan script diawali `SET NAMES utf8mb4` dan `TIME_ZONE='+00:00'`.

Script tidak disimpan utuh di memori. Generator menulis secara streaming ke tujuan yang dibutuhkan:

- **Pratinjau**: berhenti setelah sekitar 2 MB, jadi tampil dalam hitungan milidetik berapa pun besar diff-nya.
- **Total** (jumlah statement dan ukuran): dihitung di latar belakang tanpa menyimpan isi script, lalu ditampilkan menyusul. Hitungan ini batal sendiri bila opsi diubah sebelum selesai.
- **Simpan .sql**: ditulis langsung ke file.
- **Jalankan di A**: script dibuat di memori tepat sebelum dieksekusi.

Header script, termasuk waktu pembuatan, ditetapkan sekali per set opsi, jadi file yang disimpan sama persis dengan pratinjau.

### 6. Eksekusi

Script dijalankan statement demi statement. `INSERT`/`UPDATE`/`DELETE` dikelompokkan dalam transaksi; DDL dijalankan di luar transaksi karena MySQL melakukan commit otomatis untuk DDL. Eksekusi berhenti di error pertama, transaksi DML yang sedang berjalan di-rollback, dan laporan menunjukkan statement mana yang gagal.

## Keamanan dan data sensitif

- Tidak ada eksekusi tanpa konfirmasi eksplisit (ketik nama database). Default-nya hanya pratinjau.
- Operasi yang menghapus (drop tabel) mati secara default; drop kolom dan hapus baris bisa dimatikan.
- Password koneksi hanya disimpan di Keychain macOS, tidak pernah ke file.
- Isi baris tidak pernah ditulis ke log. CLI hanya mencetak jumlah.
- Dump asli yang berisi data pasien tidak boleh masuk repo. `.gitignore` menolak semua file `.sql` kecuali fixture sintetis di `fixtures/`.

## Teknologi

| Bagian | Pilihan |
|---|---|
| Shell aplikasi | Tauri 2, build native `aarch64-apple-darwin` |
| UI | React 19 + TypeScript + Vite, CSS sendiri (mode terang/gelap) |
| Engine | Rust: parser sendiri, `xxhash` (xxh3), `similar` untuk diff baris DDL |
| Koneksi database | `mysql_async` (protokol teks, TLS opsional) |
| Kredensial | `keyring` dengan Keychain macOS |

## Status terhadap rencana awal

| Fase | Isi | Status |
|---|---|---|
| M0 | Spec normalisasi, fixture | Selesai: [spec-m0.md](spec-m0.md), fixture klinik sintetis |
| M1 | Scaffold, reader streaming | Selesai |
| M2 | Parser `CREATE TABLE`, schema diff dari file | Selesai |
| M3 | Sumber live | Selesai (lewat `SHOW CREATE TABLE`) |
| M4 | Data diff berdasarkan PK | Selesai: hash per baris; belum hash per chunk di sisi server |
| M5 | Script generator, urutan FK, eksekusi | Selesai |
| M6 | UI | Versi pertama selesai |
| M7 | Keychain, koneksi, packaging | Keychain dan koneksi selesai; `.app`/`.dmg` sudah jadi tetapi belum di-sign dan di-notarize |
| M8 | Hardening | Sebagian: test regresi dan benchmark ada |

Definition of done MVP:

| Kriteria | Status |
|---|---|
| Dump 1 GB diproses dalam waktu wajar dengan RAM terbatas | 2 × 1 GB dalam 17,5 detik, RAM 1,1 GB bila perbedaannya kecil. Bila hampir semua baris berbeda, RAM bisa mencapai beberapa GB |
| Benar terhadap MySQL 8 dan MariaDB 11 | Diuji ke MariaDB 13 live dan fixture format MySQL 8. Belum diuji ke server MySQL 8 |
| Round-trip: script A→B diterapkan ke A menghasilkan B | Lulus, dua arah, dari file maupun live |
| Signed dan notarized | Belum; butuh Apple Developer ID |

## Langkah berikutnya

1. Jalankan round-trip ke MySQL 8.0, 8.4, MariaDB 10.11, dan 11 lewat `docker-compose.yml`.
2. Uji dengan dump asli (`tes_0.sql` + `tes.sql`) secara lokal, tanpa memasukkannya ke repo.
3. Kurangi memori data diff: baris yang berbeda masih disimpan utuh di memori sampai script dibuat (script sendiri sudah ditulis streaming).
4. Hash per chunk di sisi server untuk sumber live, supaya tabel besar tidak perlu ditarik utuh lewat jaringan.
5. Sign dan notarize build untuk distribusi.

Panduan instalasi, build, CLI, dan test ada di [README](../README.md).
