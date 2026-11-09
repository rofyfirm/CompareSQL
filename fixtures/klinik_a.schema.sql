/*M!999999\- enable the sandbox mode */ 
-- MariaDB dump 10.20-13.0.2-MariaDB, for osx10.23 (arm64)
--
-- Host: 127.0.0.1    Database: klinik_a
-- ------------------------------------------------------
-- Server version	13.0.2-MariaDB

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET @OLD_CHARACTER_SET_RESULTS=@@CHARACTER_SET_RESULTS */;
/*!40101 SET @OLD_COLLATION_CONNECTION=@@COLLATION_CONNECTION */;
/*!40101 SET NAMES utf8mb4 */;
/*!40103 SET @OLD_TIME_ZONE=@@TIME_ZONE */;
/*!40103 SET TIME_ZONE='+00:00' */;
/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;
/*!40014 SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS=0 */;
/*!40101 SET @OLD_SQL_MODE=@@SQL_MODE, SQL_MODE='NO_AUTO_VALUE_ON_ZERO' */;
/*M!100616 SET @OLD_NOTE_VERBOSITY=@@NOTE_VERBOSITY, NOTE_VERBOSITY=0 */;

--
-- Table structure for table `dokter`
--

DROP TABLE IF EXISTS `dokter`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `dokter` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `poli_id` int(11) NOT NULL,
  `nama` varchar(100) NOT NULL,
  `sip` varchar(30) DEFAULT NULL,
  `aktif` tinyint(1) NOT NULL DEFAULT 1,
  PRIMARY KEY (`id`),
  KEY `idx_poli` (`poli_id`),
  CONSTRAINT `fk_dokter_poli` FOREIGN KEY (`poli_id`) REFERENCES `poli` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=5 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `kunjungan`
--

DROP TABLE IF EXISTS `kunjungan`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `kunjungan` (
  `id` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  `pasien_id` bigint(20) unsigned NOT NULL,
  `dokter_id` int(11) NOT NULL,
  `tanggal` datetime NOT NULL,
  `keluhan` varchar(255) DEFAULT NULL,
  `biaya` decimal(12,2) NOT NULL DEFAULT 0.00,
  PRIMARY KEY (`id`),
  KEY `idx_pasien` (`pasien_id`),
  KEY `idx_dokter` (`dokter_id`),
  CONSTRAINT `fk_kunj_dokter` FOREIGN KEY (`dokter_id`) REFERENCES `dokter` (`id`),
  CONSTRAINT `fk_kunj_pasien` FOREIGN KEY (`pasien_id`) REFERENCES `pasien` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=6 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `log_akses`
--

DROP TABLE IF EXISTS `log_akses`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `log_akses` (
  `waktu` datetime NOT NULL,
  `pengguna` varchar(50) NOT NULL,
  `aksi` varchar(50) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `pasien`
--

DROP TABLE IF EXISTS `pasien`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `pasien` (
  `id` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  `no_rm` varchar(20) NOT NULL,
  `nama` varchar(100) NOT NULL,
  `tgl_lahir` date DEFAULT NULL,
  `jk` enum('L','P') NOT NULL,
  `alamat` text DEFAULT NULL,
  `telepon` varchar(20) DEFAULT NULL,
  `foto` blob DEFAULT NULL,
  `flags` bit(4) NOT NULL DEFAULT b'0',
  `catatan` longtext CHARACTER SET utf8mb4 COLLATE utf8mb4_bin DEFAULT NULL CHECK (json_valid(`catatan`)),
  `dibuat` timestamp NOT NULL DEFAULT current_timestamp(),
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_no_rm` (`no_rm`),
  KEY `idx_nama` (`nama`)
) ENGINE=InnoDB AUTO_INCREMENT=6 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `poli`
--

DROP TABLE IF EXISTS `poli`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `poli` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `kode` varchar(10) NOT NULL,
  `nama` varchar(100) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uk_kode` (`kode`)
) ENGINE=InnoDB AUTO_INCREMENT=4 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `resep_item`
--

DROP TABLE IF EXISTS `resep_item`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `resep_item` (
  `kunjungan_id` bigint(20) unsigned NOT NULL,
  `urut` int(11) NOT NULL,
  `obat` varchar(100) NOT NULL,
  `dosis` varchar(50) DEFAULT NULL,
  PRIMARY KEY (`kunjungan_id`,`urut`),
  CONSTRAINT `fk_resep_kunj` FOREIGN KEY (`kunjungan_id`) REFERENCES `kunjungan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `tarif_lama`
--

DROP TABLE IF EXISTS `tarif_lama`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `tarif_lama` (
  `id` int(11) NOT NULL,
  `nama` varchar(50) DEFAULT NULL,
  `harga` int(11) DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Temporary table structure for view `v_dokter_aktif`
--

DROP TABLE IF EXISTS `v_dokter_aktif`;
/*!50001 DROP VIEW IF EXISTS `v_dokter_aktif`*/;
SET @saved_cs_client     = @@character_set_client;
SET character_set_client = utf8mb4;
/*!50001 CREATE VIEW `v_dokter_aktif` AS SELECT
 NULL AS `id`,
 NULL AS `nama` */;
SET character_set_client = @saved_cs_client;

--
-- Final view structure for view `v_dokter_aktif`
--

/*!50001 DROP VIEW IF EXISTS `v_dokter_aktif`*/;
/*!50001 SET @saved_cs_client          = @@character_set_client */;
/*!50001 SET @saved_cs_results         = @@character_set_results */;
/*!50001 SET @saved_col_connection     = @@collation_connection */;
/*!50001 SET character_set_client      = utf8mb4 */;
/*!50001 SET character_set_results     = utf8mb4 */;
/*!50001 SET collation_connection      = utf8mb4_uca1400_ai_ci */;
/*!50001 CREATE ALGORITHM=UNDEFINED */
/*!50013 DEFINER=`selisih`@`localhost` SQL SECURITY DEFINER */
/*!50001 VIEW `v_dokter_aktif` AS select `dokter`.`id` AS `id`,`dokter`.`nama` AS `nama` from `dokter` where `dokter`.`aktif` = 1 */;
/*!50001 SET character_set_client      = @saved_cs_client */;
/*!50001 SET character_set_results     = @saved_cs_results */;
/*!50001 SET collation_connection      = @saved_col_connection */;
/*!40103 SET TIME_ZONE=@OLD_TIME_ZONE */;

/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;
/*!40014 SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS */;
/*!40014 SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS */;
/*!40101 SET CHARACTER_SET_CLIENT=@OLD_CHARACTER_SET_CLIENT */;
/*!40101 SET CHARACTER_SET_RESULTS=@OLD_CHARACTER_SET_RESULTS */;
/*!40101 SET COLLATION_CONNECTION=@OLD_COLLATION_CONNECTION */;
/*M!100616 SET NOTE_VERBOSITY=@OLD_NOTE_VERBOSITY */;

-- Dump completed
