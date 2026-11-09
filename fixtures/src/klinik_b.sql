-- Fixture sintetis sisi B (versi "baru"). Semua nama dan data karangan.
CREATE TABLE poli (
  id INT NOT NULL AUTO_INCREMENT PRIMARY KEY,
  kode VARCHAR(10) NOT NULL,
  nama VARCHAR(100) NOT NULL,
  lantai TINYINT NULL,
  UNIQUE KEY uk_kode (kode)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE dokter (
  id INT NOT NULL AUTO_INCREMENT PRIMARY KEY,
  poli_id INT NOT NULL,
  nama VARCHAR(100) NOT NULL,
  sip VARCHAR(50) NULL,
  aktif TINYINT(1) NOT NULL DEFAULT 1,
  KEY idx_poli (poli_id),
  CONSTRAINT fk_dokter_poli FOREIGN KEY (poli_id) REFERENCES poli (id) ON UPDATE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE pasien (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  no_rm VARCHAR(20) NOT NULL,
  nama VARCHAR(100) NOT NULL,
  jk ENUM('L','P') NOT NULL,
  tgl_lahir DATE NULL,
  alamat TEXT NULL,
  email VARCHAR(100) NULL,
  foto BLOB NULL,
  flags BIT(4) NOT NULL DEFAULT b'0000',
  catatan JSON NULL,
  dibuat TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  UNIQUE KEY uk_no_rm (no_rm),
  KEY idx_nama (nama, tgl_lahir),
  KEY idx_tgl (tgl_lahir)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE kunjungan (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  pasien_id BIGINT UNSIGNED NOT NULL,
  dokter_id INT NOT NULL,
  tanggal DATETIME NOT NULL,
  keluhan VARCHAR(255) NULL,
  biaya DECIMAL(14,2) NOT NULL DEFAULT 0.00,
  status ENUM('baru','selesai') NOT NULL DEFAULT 'baru',
  KEY idx_pasien (pasien_id),
  KEY idx_dokter (dokter_id),
  CONSTRAINT fk_kunj_pasien FOREIGN KEY (pasien_id) REFERENCES pasien (id) ON DELETE CASCADE,
  CONSTRAINT fk_kunj_dokter FOREIGN KEY (dokter_id) REFERENCES dokter (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci COMMENT='Kunjungan pasien';

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

CREATE TABLE tarif (
  id INT NOT NULL AUTO_INCREMENT PRIMARY KEY,
  poli_id INT NOT NULL,
  nama VARCHAR(100) NOT NULL,
  harga DECIMAL(12,2) NOT NULL,
  KEY idx_tarif_poli (poli_id),
  CONSTRAINT fk_tarif_poli FOREIGN KEY (poli_id) REFERENCES poli (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

INSERT INTO poli (id, kode, nama, lantai) VALUES
 (1, 'UMUM', 'Poli Umum', 1), (2, 'GIGI', 'Poli Gigi & Mulut', 2), (3, 'ANAK', 'Poli Anak', NULL), (4, 'MATA', 'Poli Mata', 3);

INSERT INTO dokter (id, poli_id, nama, sip, aktif) VALUES
 (1, 1, 'dr. Andi Contoh', 'SIP-001/2024', 1),
 (2, 2, 'drg. Budi Contoh', 'SIP-002', 1),
 (3, 3, 'dr. Citra Contoh, Sp.A', NULL, 1),
 (5, 4, 'dr. Eka Contoh, Sp.M', 'SIP-005', 1);

INSERT INTO pasien (id, no_rm, nama, jk, tgl_lahir, alamat, email, foto, flags, catatan, dibuat) VALUES
 (1, 'RM0001', 'Pasien Satu', 'L', '1990-01-15', 'Jl. Contoh No. 1', 'satu@contoh.test', NULL, b'0001', '{"alergi": ["debu"]}', '2024-01-01 08:00:00'),
 (2, 'RM0002', 'Pasien Dua "Ibu"', 'P', '1985-06-30', 'Jl. O''Brien 2\\B', NULL, 0x00FF10, b'1010', NULL, '2024-01-02 09:30:00'),
 (3, 'RM0003', 'Pasien Tiga', 'L', NULL, NULL, NULL, NULL, b'0000', NULL, '2024-01-03 10:00:00'),
 (5, 'RM0005', 'Pasien Lima 🙂', 'L', '1977-03-03', 'Jl. Ü-mlaut 5', NULL, 0x89504E470D0A1A0A00, b'0000', NULL, '2024-01-05 12:00:00'),
 (6, 'RM0006', 'Pasien Enam', 'P', '2010-07-07', 'Tab\there', NULL, NULL, b'0100', NULL, '2024-03-01 07:00:00');

INSERT INTO kunjungan (id, pasien_id, dokter_id, tanggal, keluhan, biaya, status) VALUES
 (1, 1, 1, '2024-02-01 08:00:00', 'Demam', 55000.00, 'selesai'),
 (2, 2, 2, '2024-02-01 09:00:00', 'Sakit gigi', 150000.00, 'baru'),
 (3, 3, 3, '2024-02-02 10:00:00', NULL, 75000.00, 'baru'),
 (5, 5, 1, '2024-02-04 12:00:00', 'Batuk', 50000.50, 'baru'),
 (6, 6, 5, '2024-03-01 07:30:00', 'Mata merah', 120000.00, 'baru');

INSERT INTO resep_item (kunjungan_id, urut, obat, dosis) VALUES
 (1, 1, 'Paracetamol 500mg', '3x1 sesudah makan'),
 (2, 1, 'Amoxicillin', '3x1'),
 (5, 1, 'Sirup batuk', '3x1 sdm'),
 (6, 1, 'Tetes mata', '4x1 tetes');

INSERT INTO log_akses (waktu, pengguna, aksi) VALUES
 ('2024-02-01 08:00:00', 'admin', 'login'),
 ('2024-02-01 08:05:00', 'perawat', 'lihat pasien'),
 ('2024-02-01 09:00:00', 'admin', 'logout'),
 ('2024-03-01 07:00:00', 'admin', 'login');

INSERT INTO tarif (id, poli_id, nama, harga) VALUES
 (1, 1, 'Konsultasi umum', 50000.00), (2, 2, 'Cabut gigi', 150000.00), (3, 4, 'Periksa mata', 120000.00);
