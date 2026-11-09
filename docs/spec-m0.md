# Spec M0: normalisasi, statement, dan urutan script

Dokumen ini adalah aturan yang dipakai `selisih-core`. Test di `crates/core` menguji aturan ini.

## 1. Statement yang dibaca dari dump

| Statement | Perlakuan |
|---|---|
| `CREATE TABLE` | Di-parse penuh ke model |
| `DROP TABLE` | Hapus tabel dari model (menangani placeholder view MySQL lama) |
| `INSERT` / `REPLACE` (dengan atau tanpa daftar kolom, extended insert) | Offset dicatat, baris dibaca per tabel saat data diff |
| `CREATE VIEW/TRIGGER/PROCEDURE/FUNCTION/EVENT/SEQUENCE` | Dicatat sebagai "di luar scope", tidak dibandingkan |
| `ALTER TABLE ... DISABLE/ENABLE KEYS`, `SET`, `LOCK`, `USE`, `CREATE DATABASE` | Diabaikan |
| `ALTER TABLE` lain, `CREATE INDEX` terpisah | Diabaikan dengan peringatan |
| `CREATE TABLE ... LIKE/SELECT` | Diabaikan dengan peringatan |

Pemotongan statement: delimiter di luar string, backtick, dan komentar. Perintah klien `DELIMITER` didukung. Isi `/*!NNNNN ... */` dibaca sebagai kode; `/*M! ... */` dibaca sebagai komentar.

## 2. Normalisasi yang selalu aktif (aman secara semantik)

- Nama tipe huruf kecil; nilai `enum`/`set` tetap peka huruf.
- Alias tipe: `integer`→`int`, `bool`→`tinyint(1)`, `numeric/dec/fixed`→`decimal`, `decimal`→`decimal(10,0)`, `real`→`double`.
- `utf8` → `utf8mb3`, `utf8_*` → `utf8mb3_*`.
- Charset/collation kolom yang sama dengan default tabel dibuang. Kolom yang hanya punya `COLLATE` mendapat charset dari prefix collation.
- `DEFAULT NULL` pada kolom nullable = tanpa default (MariaDB vs MySQL untuk TEXT/BLOB).
- Default angka bertanda kutip pada kolom numerik jadi angka (`'0.00'` = `0.00`).
- `current_timestamp()`, `now()`, `CURRENT_TIMESTAMP` → `CURRENT_TIMESTAMP` (+presisi).
- Ekspresi (default, generated, CHECK, functional index): kurung luar dan introducer `_utf8mb4` dibuang, kata kunci huruf kecil.
- MariaDB JSON (`longtext ... utf8mb4_bin CHECK (json_valid(col))`) = `json`.
- `USING BTREE` dibuang (kecuali engine MEMORY), `ON DELETE/UPDATE RESTRICT|NO ACTION` = default.
- Kolom primary key selalu `NOT NULL`.
- Collation tabel kosong dianggap sama dengan collation default charset (`utf8mb4_general_ci`, `utf8mb4_0900_ai_ci`, `utf8mb4_uca1400_ai_ci`, ...).

## 3. Opsi abaikan (bisa diubah di UI)

| Opsi | Default |
|---|---|
| Lebar display integer (`int(11)` = `int`, `year(4)` = `year`) | aktif |
| Nilai `AUTO_INCREMENT` tabel | aktif |
| Nama tabel tidak peka huruf | aktif |
| `ROW_FORMAT` | aktif |
| Urutan kolom | mati |
| Komentar | mati |
| Charset & collation | mati |
| Engine | mati |

Opsi noise tabel (`PAGE_CHECKSUM`, `STATS_*`, `TRANSACTIONAL`, ...) selalu diabaikan.

## 4. Pencocokan objek

- Tabel: nama. Kolom dan index: nama, tidak peka huruf.
- Foreign key: nama, lalu definisi bila nama berbeda (nama otomatis `t_ibfk_N` beda antar server).
- CHECK: ekspresi (nama otomatis beda antar server).
- Urutan kolom: kolom bersama di luar LCS urutan A vs B dianggap "pindah".

## 5. Data diff

- Kunci: primary key B (bila semua kolomnya ada di A), lalu PK A, lalu unique index B yang semua kolomnya `NOT NULL`. Tanpa itu: seluruh baris sebagai multiset.
- Nilai dibandingkan dalam bentuk teks protokol MySQL (sama dengan isi dump): NULL berbeda dari string kosong, biner sebagai byte mentah, BIT sebagai angka desimal, TIMESTAMP dalam UTC.
- Kolom generated tidak dibandingkan dan tidak ditulis.
- Kolom yang hanya ada di B: nilai A dianggap default literal B. Default berupa ekspresi: kolom itu tidak dibandingkan (peringatan).
- Memori: 32 byte per baris A untuk hash + baris yang berbeda disimpan penuh.

## 6. Urutan script (dijalankan di A supaya sama dengan B)

1. `DROP FOREIGN KEY`: FK yang hilang/berubah, dan FK yang menyentuh kolom/index yang berubah.
2. `DROP TABLE` (hanya bila opsi aktif).
3. `CREATE TABLE` baru tanpa FK, urut topologis.
4. `ALTER TABLE` per tabel: drop CHECK, drop index/PK, drop kolom, add/modify kolom urut B (`FIRST`/`AFTER`), add PK/index, add CHECK, opsi tabel.
5. `DELETE` urut topologis terbalik (anak dulu).
6. `UPDATE` (hanya kolom yang beda) lalu `INSERT` batch, urut topologis (induk dulu).
7. `ADD FOREIGN KEY`.

Header script: `SET NAMES utf8mb4`, `TIME_ZONE='+00:00'`, `SQL_MODE='NO_AUTO_VALUE_ON_ZERO'`, dan dikembalikan di akhir. Literal: biner sebagai `0x..`, angka tanpa kutip, string di-escape gaya MySQL.

## 7. Eksekusi

Hanya bila A adalah DB live dan nama database diketik ulang. DML dikelompokkan dalam transaksi; DDL di luar transaksi. Berhenti di error pertama dan rollback transaksi DML yang sedang berjalan.
