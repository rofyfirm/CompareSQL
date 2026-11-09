-- MySQL dump 10.13  Distrib 8.0.36, for macos14 (arm64)
--
-- Host: localhost    Database: klinik_a
-- ------------------------------------------------------
-- Server version	8.0.36
-- Fixture tulisan tangan: schema klinik_a dalam format SHOW CREATE TABLE MySQL 8.

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET NAMES utf8mb4 */;
/*!40103 SET @OLD_TIME_ZONE=@@TIME_ZONE */;
/*!40103 SET TIME_ZONE='+00:00' */;
/*!40014 SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS=0 */;
/*!40101 SET @OLD_SQL_MODE=@@SQL_MODE, SQL_MODE='NO_AUTO_VALUE_ON_ZERO' */;
/*!40111 SET @OLD_SQL_NOTES=@@SQL_NOTES, SQL_NOTES=0 */;

DROP TABLE IF EXISTS `dokter`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `dokter` (
  `id` int NOT NULL AUTO_INCREMENT,
  `poli_id` int NOT NULL,
  `nama` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `sip` varchar(30) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `aktif` tinyint(1) NOT NULL DEFAULT '1',
  PRIMARY KEY (`id`),
  KEY `idx_poli` (`poli_id`),
  CONSTRAINT `fk_dokter_poli` FOREIGN KEY (`poli_id`) REFERENCES `poli` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=5 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

DROP TABLE IF EXISTS `kunjungan`;
CREATE TABLE `kunjungan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pasien_id` bigint unsigned NOT NULL,
  `dokter_id` int NOT NULL,
  `tanggal` datetime NOT NULL,
  `keluhan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `biaya` decimal(12,2) NOT NULL DEFAULT '0.00',
  PRIMARY KEY (`id`),
  KEY `idx_pasien` (`pasien_id`),
  KEY `idx_dokter` (`dokter_id`),
  CONSTRAINT `fk_kunj_dokter` FOREIGN KEY (`dokter_id`) REFERENCES `dokter` (`id`),
  CONSTRAINT `fk_kunj_pasien` FOREIGN KEY (`pasien_id`) REFERENCES `pasien` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=6 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

DROP TABLE IF EXISTS `log_akses`;
CREATE TABLE `log_akses` (
  `waktu` datetime NOT NULL,
  `pengguna` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `aksi` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

DROP TABLE IF EXISTS `pasien`;
CREATE TABLE `pasien` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `no_rm` varchar(20) COLLATE utf8mb4_unicode_ci NOT NULL,
  `nama` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `tgl_lahir` date DEFAULT NULL,
  `jk` enum('L','P') COLLATE utf8mb4_unicode_ci NOT NULL,
  `alamat` text COLLATE utf8mb4_unicode_ci,
  `telepon` varchar(20) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `foto` blob,
  `flags` bit(4) NOT NULL DEFAULT b'0',
  `catatan` json DEFAULT NULL,
  `dibuat` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_no_rm` (`no_rm`),
  KEY `idx_nama` (`nama`)
) ENGINE=InnoDB AUTO_INCREMENT=6 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

DROP TABLE IF EXISTS `poli`;
CREATE TABLE `poli` (
  `id` int NOT NULL AUTO_INCREMENT,
  `kode` varchar(10) COLLATE utf8mb4_unicode_ci NOT NULL,
  `nama` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_kode` (`kode`)
) ENGINE=InnoDB AUTO_INCREMENT=4 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

DROP TABLE IF EXISTS `resep_item`;
CREATE TABLE `resep_item` (
  `kunjungan_id` bigint unsigned NOT NULL,
  `urut` int NOT NULL,
  `obat` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `dosis` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  PRIMARY KEY (`kunjungan_id`,`urut`),
  CONSTRAINT `fk_resep_kunj` FOREIGN KEY (`kunjungan_id`) REFERENCES `kunjungan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

DROP TABLE IF EXISTS `tarif_lama`;
CREATE TABLE `tarif_lama` (
  `id` int NOT NULL,
  `nama` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `harga` int DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

--
-- Temporary view structure for view `v_dokter_aktif`
--

DROP TABLE IF EXISTS `v_dokter_aktif`;
/*!50001 DROP VIEW IF EXISTS `v_dokter_aktif`*/;
SET @saved_cs_client     = @@character_set_client;
/*!50503 SET character_set_client = utf8mb4 */;
/*!50001 CREATE VIEW `v_dokter_aktif` AS SELECT
 1 AS `id`,
 1 AS `nama`*/;
SET character_set_client = @saved_cs_client;

/*!50001 DROP VIEW IF EXISTS `v_dokter_aktif`*/;
/*!50001 SET @saved_cs_client          = @@character_set_client */;
/*!50001 CREATE ALGORITHM=UNDEFINED */
/*!50013 DEFINER=`root`@`localhost` SQL SECURITY DEFINER */
/*!50001 VIEW `v_dokter_aktif` AS select `dokter`.`id` AS `id`,`dokter`.`nama` AS `nama` from `dokter` where (`dokter`.`aktif` = 1) */;
/*!50001 SET character_set_client      = @saved_cs_client */;

/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;
/*!40014 SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS */;
/*!40103 SET TIME_ZONE=@OLD_TIME_ZONE */;
