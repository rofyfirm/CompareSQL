// Pembungkus command Tauri. Di browser biasa (tanpa Tauri) memakai mock,
// supaya UI bisa dikembangkan dengan `npm run dev`.

import type {
  CompareOptions,
  CompareSummary,
  ConnectionProfile,
  ExecReport,
  ProgressEvent,
  ScriptOptions,
  ScriptPreview,
  ScriptTotals,
  SideInput,
  TableDetail,
} from "./types";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

let invokeImpl: Promise<Invoke> | null = null;

function getInvoke(): Promise<Invoke> {
  if (!invokeImpl) {
    invokeImpl = inTauri
      ? import("@tauri-apps/api/core").then((m) => m.invoke as Invoke)
      : import("./mock").then((m) => m.mockInvoke as Invoke);
  }
  return invokeImpl;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const inv = await getInvoke();
  try {
    return await inv<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

export const api = {
  compare: (input: { a: SideInput; b: SideInput; compareData: boolean; options: CompareOptions }) =>
    call<CompareSummary>("compare", { input }),
  cancel: () => call<void>("cancel"),
  tableDetail: (name: string) => call<TableDetail>("table_detail", { name }),
  generateScript: (options: ScriptOptions) => call<ScriptPreview>("generate_script", { options }),
  scriptTotals: (id: number) => call<ScriptTotals>("script_totals", { id }),
  saveScript: (path: string) => call<void>("save_script", { path }),
  executeScript: (confirm: string) => call<ExecReport>("execute_script", { confirm }),
  listConnections: () => call<ConnectionProfile[]>("list_connections"),
  saveConnection: (profile: ConnectionProfile, password: string | null) =>
    call<ConnectionProfile>("save_connection", { profile, password }),
  deleteConnection: (id: string) => call<void>("delete_connection", { id }),
  testConnection: (profile: ConnectionProfile, password: string | null) =>
    call<string>("test_connection", { profile, password }),
  listDatabases: (id: string) => call<string[]>("list_databases", { id }),
};

export async function onProgress(cb: (p: ProgressEvent) => void): Promise<() => void> {
  if (inTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<ProgressEvent>("progress", (e) => cb(e.payload));
  }
  const m = await import("./mock");
  return m.mockOnProgress(cb);
}

const sqlFilters = [{ name: "SQL dump", extensions: ["sql", "txt", "dump"] }];

export async function pickSqlFile(): Promise<string | null> {
  if (!inTauri) return "/contoh/klinik_a.sql";
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ multiple: false, directory: false, filters: sqlFilters });
  return typeof r === "string" ? r : null;
}

export async function pickSavePath(defaultName: string): Promise<string | null> {
  if (!inTauri) return `/contoh/${defaultName}`;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return save({ defaultPath: defaultName, filters: [{ name: "SQL", extensions: ["sql"] }] });
}
