// Backend tiruan untuk `npm run dev` di browser biasa. Data karangan.
import type {
  CompareSummary,
  ConnectionProfile,
  ExecReport,
  ProgressEvent,
  ScriptPreview,
  ScriptTotals,
  TableDetail,
} from "./types";

const listeners = new Set<(p: ProgressEvent) => void>();
export function mockOnProgress(cb: (p: ProgressEvent) => void) {
  listeners.add(cb);
  return () => listeners.delete(cb);
}
const emit = (p: ProgressEvent) => listeners.forEach((l) => l(p));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let scriptId = 0;

let connections: ConnectionProfile[] = [
  { id: "c1", name: "Lokal MariaDB", host: "127.0.0.1", port: 3306, user: "root", useTls: false, acceptInvalidCerts: false, hasPassword: true },
];

const summary: CompareSummary = {
  labelA: "klinik_a.sql",
  labelB: "klinik_b.sql",
  versionA: null,
  versionB: null,
  elapsedMs: 42,
  dataCompared: true,
  dataSkipped: null,
  warnings: [],
  aIsLive: true,
  aDatabase: "klinik_a",
  skippedA: [{ kind: "view", name: "v_dokter_aktif" }],
  skippedB: [],
  tables: [
    {
      name: "dokter", nameA: "dokter", nameB: "dokter", status: "modified",
      changes: [
        { category: "column", action: "modified", name: "sip", detail: "tipe varchar(30) → varchar(50)" },
        { category: "foreign_key", action: "modified", name: "fk_dokter_poli", detail: "ON UPDATE - → CASCADE" },
      ],
      data: { rowsA: 4, rowsB: 4, same: 2, changed: 1, onlyA: 1, onlyB: 1, keyMode: "primaryKey", warnings: [] },
      dataError: null,
    },
    {
      name: "kunjungan", nameA: "kunjungan", nameB: "kunjungan", status: "modified",
      changes: [
        { category: "column", action: "modified", name: "biaya", detail: "tipe decimal(12,2) → decimal(14,2)" },
        { category: "column", action: "added", name: "status", detail: "enum('baru','selesai')" },
        { category: "option", action: "modified", name: "COMMENT", detail: "- → Kunjungan pasien" },
      ],
      data: { rowsA: 5, rowsB: 5, same: 3, changed: 1, onlyA: 1, onlyB: 1, keyMode: "primaryKey", warnings: [] },
      dataError: null,
    },
    {
      name: "log_akses", nameA: "log_akses", nameB: "log_akses", status: "same", changes: [],
      data: { rowsA: 4, rowsB: 4, same: 3, changed: 0, onlyA: 1, onlyB: 1, keyMode: "fullRow", warnings: ["tidak ada primary key / unique NOT NULL: dibandingkan per baris utuh"] },
      dataError: null,
    },
    {
      name: "pasien", nameA: "pasien", nameB: "pasien", status: "modified",
      changes: [
        { category: "column", action: "removed", name: "telepon", detail: "varchar(20)" },
        { category: "column", action: "added", name: "email", detail: "varchar(100)" },
        { category: "column", action: "moved", name: "tgl_lahir", detail: "urutan kolom berubah" },
        { category: "index", action: "modified", name: "idx_nama", detail: "KEY `idx_nama` (`nama`) → KEY `idx_nama` (`nama`,`tgl_lahir`)" },
        { category: "index", action: "added", name: "idx_tgl", detail: "KEY `idx_tgl` (`tgl_lahir`)" },
      ],
      data: { rowsA: 5, rowsB: 5, same: 1, changed: 3, onlyA: 1, onlyB: 1, keyMode: "primaryKey", warnings: [] },
      dataError: null,
    },
    {
      name: "poli", nameA: "poli", nameB: "poli", status: "modified",
      changes: [{ category: "column", action: "added", name: "lantai", detail: "tinyint(4)" }],
      data: { rowsA: 3, rowsB: 4, same: 1, changed: 2, onlyA: 0, onlyB: 1, keyMode: "primaryKey", warnings: [] },
      dataError: null,
    },
    {
      name: "resep_item", nameA: "resep_item", nameB: "resep_item", status: "same", changes: [],
      data: { rowsA: 4, rowsB: 4, same: 2, changed: 1, onlyA: 1, onlyB: 1, keyMode: "primaryKey", warnings: [] },
      dataError: null,
    },
    {
      name: "tarif", nameA: null, nameB: "tarif", status: "onlyInB", changes: [],
      data: { rowsA: 0, rowsB: 3, same: 0, changed: 0, onlyA: 0, onlyB: 3, keyMode: "primaryKey", warnings: [] },
      dataError: null,
    },
    { name: "tarif_lama", nameA: "tarif_lama", nameB: null, status: "onlyInA", changes: [], data: null, dataError: null },
    {
      name: "wilayah", nameA: "wilayah", nameB: "wilayah", status: "same", changes: [],
      data: { rowsA: 120, rowsB: 120, same: 120, changed: 0, onlyA: 0, onlyB: 0, keyMode: "primaryKey", warnings: [] },
      dataError: null,
    },
  ],
};

const createA = `CREATE TABLE \`pasien\` (
  \`id\` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  \`no_rm\` varchar(20) NOT NULL,
  \`nama\` varchar(100) NOT NULL,
  \`tgl_lahir\` date NULL,
  \`jk\` enum('L','P') NOT NULL,
  \`alamat\` text NULL,
  \`telepon\` varchar(20) NULL,
  \`foto\` blob NULL,
  PRIMARY KEY (\`id\`),
  UNIQUE KEY \`uk_no_rm\` (\`no_rm\`),
  KEY \`idx_nama\` (\`nama\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci`;

const createB = `CREATE TABLE \`pasien\` (
  \`id\` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  \`no_rm\` varchar(20) NOT NULL,
  \`nama\` varchar(100) NOT NULL,
  \`jk\` enum('L','P') NOT NULL,
  \`tgl_lahir\` date NULL,
  \`alamat\` text NULL,
  \`email\` varchar(100) NULL,
  \`foto\` blob NULL,
  PRIMARY KEY (\`id\`),
  UNIQUE KEY \`uk_no_rm\` (\`no_rm\`),
  KEY \`idx_nama\` (\`nama\`,\`tgl_lahir\`),
  KEY \`idx_tgl\` (\`tgl_lahir\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci`;

function lineDiff(a: string, b: string) {
  // Diff kasar untuk mock saja.
  const la = a.split("\n");
  const lb = b.split("\n");
  const out: TableDetail["lines"] = [];
  let i = 0;
  let j = 0;
  while (i < la.length || j < lb.length) {
    if (la[i] === lb[j]) out.push({ kind: "same", a: la[i++], b: lb[j++] });
    else if (j < lb.length && !la.includes(lb[j])) out.push({ kind: "onlyB", a: null, b: lb[j++] });
    else if (i < la.length && !lb.includes(la[i])) out.push({ kind: "onlyA", a: la[i++], b: null });
    else out.push({ kind: "changed", a: la[i++] ?? null, b: lb[j++] ?? null });
  }
  return out;
}

const cols = ["id", "no_rm", "nama", "jk", "tgl_lahir", "alamat", "email", "foto", "flags", "catatan", "dibuat"];

const script = `-- Selisih - script sinkronisasi (mock)
SET NAMES utf8mb4;
SET TIME_ZONE='+00:00';

-- 1. Hapus foreign key
ALTER TABLE \`dokter\` DROP FOREIGN KEY \`fk_dokter_poli\`;

-- 3. Buat tabel baru
CREATE TABLE \`tarif\` (
  \`id\` int(11) NOT NULL AUTO_INCREMENT,
  \`poli_id\` int(11) NOT NULL,
  \`nama\` varchar(100) NOT NULL,
  \`harga\` decimal(12,2) NOT NULL,
  PRIMARY KEY (\`id\`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- 4. Ubah struktur tabel
ALTER TABLE \`pasien\`
  DROP INDEX \`idx_nama\`,
  DROP COLUMN \`telepon\`,
  MODIFY COLUMN \`tgl_lahir\` date NULL AFTER \`jk\`,
  ADD COLUMN \`email\` varchar(100) NULL AFTER \`alamat\`,
  ADD KEY \`idx_nama\` (\`nama\`,\`tgl_lahir\`);

-- 5. Hapus baris (anak dulu)
DELETE FROM \`kunjungan\` WHERE \`id\` IN (4);

-- 6. Ubah dan tambah baris (induk dulu)
UPDATE \`pasien\` SET \`nama\`='Pasien Dua "Ibu"', \`flags\`=10 WHERE \`id\`=2;
INSERT INTO \`poli\` (\`id\`,\`kode\`,\`nama\`,\`lantai\`) VALUES
(4,'MATA','Poli Mata',3);

-- 7. Tambah foreign key
ALTER TABLE \`dokter\`
  ADD CONSTRAINT \`fk_dokter_poli\` FOREIGN KEY (\`poli_id\`) REFERENCES \`poli\` (\`id\`) ON UPDATE CASCADE;
`;

export async function mockInvoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  switch (cmd) {
    case "compare": {
      const phases = ["Membaca A", "Membaca B", "Membandingkan data"];
      for (const [k, p] of phases.entries()) {
        for (let i = 0; i <= 10; i++) {
          emit({ phase: p, detail: k === 2 ? summary.tables[i % summary.tables.length].name : "klinik.sql", done: i, total: 10 });
          await sleep(25);
        }
      }
      return summary;
    }
    case "cancel":
      return null;
    case "table_detail": {
      const name = args.name as string;
      const t = summary.tables.find((x) => x.name === name)!;
      const a = t.nameA ? createA.replaceAll("pasien", name) : null;
      const b = t.nameB ? createB.replaceAll("pasien", name) : null;
      return {
        name,
        createA: a,
        createB: b,
        lines: lineDiff(a ?? "", b ?? "").filter((l) => l.a !== "" || l.b !== ""),
        data: t.data
          ? {
              columns: cols,
              keyColumns: [0],
              keyMode: t.data.keyMode,
              missingInA: [6],
              ignoredColumns: [],
              changed: [
                {
                  a: ["1", "RM0001", "Pasien Satu", "L", "1990-01-15", "Jl. Contoh No. 1", null, null, "1", '{"alergi": []}', "2024-01-01 01:00:00"],
                  b: ["1", "RM0001", "Pasien Satu", "L", "1990-01-15", "Jl. Contoh No. 1", "satu@contoh.test", null, "1", '{"alergi": ["debu"]}', "2024-01-01 01:00:00"],
                  diff: [6, 9],
                },
                {
                  a: ["2", "RM0002", "Pasien Dua", "P", "1985-06-30", "Jl. O'Brien 2\\B", null, "0x00FF10", "0", null, "2024-01-02 02:30:00"],
                  b: ["2", "RM0002", 'Pasien Dua "Ibu"', "P", "1985-06-30", "Jl. O'Brien 2\\B", null, "0x00FF10", "10", null, "2024-01-02 02:30:00"],
                  diff: [2, 8],
                },
              ],
              onlyA: [["4", "RM0004", "Pasien Empat", "P", "2001-12-01", "Baris1\nBaris2", null, null, "2", '{"catatan": "kontrol"}', "2024-01-04 04:15:00"]],
              onlyB: [["6", "RM0006", "Pasien Enam", "P", "2010-07-07", "Tab\there", null, null, "4", null, "2024-03-01 00:00:00"]],
              changedTotal: t.data.changed,
              onlyATotal: t.data.onlyA,
              onlyBTotal: t.data.onlyB,
              sampleLimit: 300,
            }
          : null,
      } satisfies TableDetail;
    }
    case "generate_script":
      await sleep(150);
      return {
        id: ++scriptId,
        preview: script,
        truncated: false,
        totalBytes: script.length,
        statements: 9,
        warnings: ["pasien: kolom `telepon` di-drop, datanya hilang"],
      } satisfies ScriptPreview;
    case "script_totals":
      return { statements: 9, totalBytes: script.length, warnings: ["pasien: kolom `telepon` di-drop, datanya hilang"] } satisfies ScriptTotals;
    case "save_script":
      return null;
    case "execute_script": {
      for (let i = 0; i <= 9; i++) {
        emit({ phase: "Menjalankan script", detail: "", done: i, total: 9 });
        await sleep(60);
      }
      return { total: 9, executed: 9, error: null, elapsedMs: 540 } satisfies ExecReport;
    }
    case "list_connections":
      return connections;
    case "save_connection": {
      const p = { ...(args.profile as ConnectionProfile) };
      if (!p.id) p.id = `c${Date.now()}`;
      p.hasPassword = !!args.password || p.hasPassword;
      connections = [...connections.filter((c) => c.id !== p.id), p];
      return p;
    }
    case "delete_connection":
      connections = connections.filter((c) => c.id !== args.id);
      return null;
    case "test_connection":
      await sleep(300);
      return "11.4.2-MariaDB";
    case "list_databases":
      return ["klinik_a", "klinik_b", "klinik_staging"];
    default:
      throw new Error(`mock: command tidak dikenal ${cmd}`);
  }
}
