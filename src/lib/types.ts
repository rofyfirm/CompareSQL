// Tipe data yang sama dengan serialisasi command Rust (camelCase).

export type Status = "same" | "modified" | "onlyInA" | "onlyInB";
export type KeyMode = "primaryKey" | "uniqueKey" | "fullRow";

export interface Change {
  category: "column" | "primary_key" | "index" | "foreign_key" | "check" | "option";
  action: "added" | "removed" | "modified" | "moved";
  name: string;
  detail: string;
}

export interface DataSummary {
  rowsA: number;
  rowsB: number;
  same: number;
  changed: number;
  onlyA: number;
  onlyB: number;
  keyMode: KeyMode | null;
  warnings: string[];
}

export interface TableSummary {
  name: string;
  nameA: string | null;
  nameB: string | null;
  status: Status;
  changes: Change[];
  data: DataSummary | null;
  dataError: string | null;
}

export interface SkippedObject {
  kind: string;
  name: string;
}

export interface CompareSummary {
  labelA: string;
  labelB: string;
  versionA: string | null;
  versionB: string | null;
  elapsedMs: number;
  dataCompared: boolean;
  dataSkipped: string | null;
  warnings: string[];
  tables: TableSummary[];
  skippedA: SkippedObject[];
  skippedB: SkippedObject[];
  aIsLive: boolean;
  aDatabase: string | null;
}

export interface DiffLine {
  kind: "same" | "changed" | "onlyA" | "onlyB";
  a: string | null;
  b: string | null;
}

export type CellText = string | null;

export interface ChangedRowView {
  a: CellText[];
  b: CellText[];
  diff: number[];
}

export interface DataDetail {
  columns: string[];
  keyColumns: number[];
  keyMode: KeyMode | null;
  missingInA: number[];
  ignoredColumns: number[];
  changed: ChangedRowView[];
  onlyA: CellText[][];
  onlyB: CellText[][];
  changedTotal: number;
  onlyATotal: number;
  onlyBTotal: number;
  sampleLimit: number;
}

export interface TableDetail {
  name: string;
  createA: string | null;
  createB: string | null;
  lines: DiffLine[];
  data: DataDetail | null;
}

export interface CompareOptions {
  ignoreIntDisplayWidth: boolean;
  ignoreComments: boolean;
  ignoreCharsetCollation: boolean;
  ignoreEngine: boolean;
  ignoreRowFormat: boolean;
  ignoreColumnOrder: boolean;
  ignoreTableNameCase: boolean;
  ignoreAutoIncrement: boolean;
}

export const defaultCompareOptions: CompareOptions = {
  ignoreIntDisplayWidth: true,
  ignoreComments: false,
  ignoreCharsetCollation: false,
  ignoreEngine: false,
  ignoreRowFormat: true,
  ignoreColumnOrder: false,
  ignoreTableNameCase: true,
  ignoreAutoIncrement: true,
};

export interface ScriptOptions {
  includeSchema: boolean;
  includeData: boolean;
  dropTables: boolean;
  dropColumns: boolean;
  deleteRows: boolean;
  disableFkChecks: boolean;
  tables: string[] | null;
  batchRows: number;
  maxStatementBytes: number;
}

export const defaultScriptOptions: ScriptOptions = {
  includeSchema: true,
  includeData: true,
  dropTables: false,
  dropColumns: true,
  deleteRows: true,
  disableFkChecks: false,
  tables: null,
  batchRows: 500,
  maxStatementBytes: 1 << 20,
};

export interface ScriptPreview {
  id: number;
  preview: string;
  truncated: boolean;
  /** null selama total masih dihitung; lihat api.scriptTotals. */
  totalBytes: number | null;
  statements: number | null;
  warnings: string[];
}

export interface ScriptTotals {
  statements: number;
  totalBytes: number;
  warnings: string[];
}

export interface ExecReport {
  total: number;
  executed: number;
  error: { index: number; statement: string; message: string; rolledBack: boolean } | null;
  elapsedMs: number;
}

export interface ConnectionProfile {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
  useTls: boolean;
  acceptInvalidCerts: boolean;
  hasPassword?: boolean;
}

export type SideInput =
  | { kind: "file"; schemaPath: string; dataPath: string | null }
  | { kind: "live"; connectionId: string; database: string };

export interface ProgressEvent {
  phase: string;
  detail: string;
  done: number;
  total: number;
}
