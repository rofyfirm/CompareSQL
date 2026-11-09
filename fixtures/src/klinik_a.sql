-- Fixture sintetis sisi A. Semua nama dan data karangan.
CREATE TABLE poli (
  id INT NOT NULL AUTO_INCREMENT PRIMARY KEY,
  kode VARCHAR(10) NOT NULL,
  nama VARCHAR(100) NOT NULL,
  UNIQUE KEY uk_kode (kode)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE dokter (
  id INT NOT NULL AUTO_INCREMENT PRIMARY KEY,
  poli_id INT NOT NULL,
  nama VARCHAR(100) NOT NULL,
  sip VARCHAR(30) NULL,
  aktif TINYINT(1) NOT NULL DEFAULT 1,
  KEY idx_poli (poli_id),
  CONSTRAINT fk_dokter_poli FOREIGN KEY (poli_id) REFERENCES poli (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE pasien (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  no_rm VARCHAR(20) NOT NULL,
  nama VARCHAR(100) NOT NULL,
  tgl_lahir DATE NULL,
  jk ENUM('L','P') NOT NULL,
  alamat TEXT NULL,
  telepon VARCHAR(20) NULL,
  foto BLOB NULL,
  flags BIT(4) NOT NULL DEFAULT b'0000',
  catatan JSON NULL,
  dibuat TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  UNIQUE KEY uk_no_rm (no_rm),
  KEY idx_nama (nama)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE kunjungan (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  pasien_id BIGINT UNSIGNED NOT NULL,
  dokter_id INT NOT NULL,
  tanggal DATETIME NOT NULL,
  keluhan VARCHAR(255) NULL,
  biaya DECIMAL(12,2) NOT NULL DEFAULT 0.00,
  KEY idx_pasien (pasien_id),
  KEY idx_dokter (dokter_id),
  CONSTRAINT fk_kunj_pasien FOREIGN KEY (pasien_id) REFERENCES pasien (id) ON DELETE CASCADE,
  CONSTRAINT fk_kunj_dokter FOREIGN KEY (dokter_id) REFERENCES dokter (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE resep_item (
  kunjungan_id BIGINT UNSIGNED NOT NULL,
  urut INT NOT NULL,
  obat VARCHAR(100) NOT NULL,
  dosis VARCHAR(50) NULL,
  PRIMARY KEY (kunjungan_id, urut),
  CONSTRAINT fk_resep_kunj FOREIGN KEY (kunjungan_id) REFERENCES kunjungan (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE log_akses (
  waktu DATETIME NOT NULL,
  pengguna VARCHAR(50) NOT NULL,
  aksi VARCHAR(50) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE tarif_lama (
  id INT NOT NULL PRIMARY KEY,
  nama VARCHAR(50) NULL,
  harga INT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE VIEW v_dokter_aktif AS SELECT id, nama FROM dokter WHERE aktif = 1;

INSERT INTO poli (id, kode, nama) VALUES
 (1, 'UMUM', 'Poli Umum'), (2, 'GIGI', 'Poli Gigi'), (3, 'ANAK', 'Poli Anak');

INSERT INTO dokter (id, poli_id, nama, sip, aktif) VALUES
 (1, 1, 'dr. Andi Contoh', 'SIP-001', 1),
 (2, 2, 'drg. Budi Contoh', 'SIP-002', 1),
 (3, 3, 'dr. Citra Contoh, Sp.A', NULL, 1),
 (4, 1, 'dr. Dimas Contoh', 'SIP-004', 0);

INSERT INTO pasien (id, no_rm, nama, tgl_lahir, jk, alamat, telepon, foto, flags, catatan, dibuat) VALUES
 (1, 'RM0001', 'Pasien Satu', '1990-01-15', 'L', 'Jl. Contoh No. 1', '0811000001', NULL, b'0001', '{"alergi": []}', '2024-01-01 08:00:00'),
 (2, 'RM0002', 'Pasien Dua', '1985-06-30', 'P', 'Jl. O''Brien 2\\B', '0811000002', 0x00FF10, b'0000', NULL, '2024-01-02 09:30:00'),
 (3, 'RM0003', 'Pasien Tiga', NULL, 'L', NULL, NULL, NULL, b'0000', NULL, '2024-01-03 10:00:00'),
 (4, 'RM0004', 'Pasien Empat', '2001-12-01', 'P', 'Baris1\nBaris2', '0811000004', NULL, b'0010', '{"catatan": "kontrol"}', '2024-01-04 11:15:00'),
 (5, 'RM0005', 'Pasien Lima', '1977-03-03', 'L', 'Jl. Ü-mlaut 5', NULL, NULL, b'0000', NULL, '2024-01-05 12:00:00');

INSERT INTO kunjungan (id, pasien_id, dokter_id, tanggal, keluhan, biaya) VALUES
 (1, 1, 1, '2024-02-01 08:00:00', 'Demam', 50000.00),
 (2, 2, 2, '2024-02-01 09:00:00', 'Sakit gigi', 150000.00),
 (3, 3, 3, '2024-02-02 10:00:00', NULL, 75000.00),
 (4, 4, 4, '2024-02-03 11:00:00', 'Kontrol', 0.00),
 (5, 5, 1, '2024-02-04 12:00:00', 'Batuk', 50000.50);

INSERT INTO resep_item (kunjungan_id, urut, obat, dosis) VALUES
 (1, 1, 'Paracetamol 500mg', '3x1'),
 (1, 2, 'Vitamin C', '1x1'),
 (2, 1, 'Amoxicillin', '3x1'),
 (5, 1, 'Sirup batuk', '3x1 sdm');

INSERT INTO log_akses (waktu, pengguna, aksi) VALUES
 ('2024-02-01 08:00:00', 'admin', 'login'),
 ('2024-02-01 08:00:00', 'admin', 'login'),
 ('2024-02-01 08:05:00', 'perawat', 'lihat pasien'),
 ('2024-02-01 09:00:00', 'admin', 'logout');

INSERT INTO tarif_lama (id, nama, harga) VALUES (1, 'Konsultasi', 50000), (2, 'Cabut gigi', 150000);
