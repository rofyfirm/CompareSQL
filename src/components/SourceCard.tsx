import { useEffect, useState } from "react";
import { api, pickSqlFile } from "../lib/api";
import type { ConnectionProfile, SideInput } from "../lib/types";

export interface SideState {
  mode: "file" | "live";
  schemaPath: string;
  dataPath: string;
  connectionId: string;
  database: string;
}

export const emptySide: SideState = { mode: "file", schemaPath: "", dataPath: "", connectionId: "", database: "" };

export function toInput(s: SideState): SideInput {
  return s.mode === "file"
    ? { kind: "file", schemaPath: s.schemaPath, dataPath: s.dataPath || null }
    : { kind: "live", connectionId: s.connectionId, database: s.database };
}

export function sideReady(s: SideState): boolean {
  return s.mode === "file" ? !!s.schemaPath : !!s.connectionId && !!s.database;
}

interface Props {
  side: "a" | "b";
  value: SideState;
  onChange: (v: SideState) => void;
  connections: ConnectionProfile[];
  onManageConnections: () => void;
}

function PathRow({ label, value, placeholder, onPick, onClear }: {
  label: string;
  value: string;
  placeholder: string;
  onPick: () => void;
  onClear?: () => void;
}) {
  return (
    <div className="path-row">
      <label>{label}</label>
      <div className={`path ${value ? "" : "empty"}`} title={value}>
        {value ? <bdi>{value}</bdi> : placeholder}
      </div>
      <div style={{ display: "flex", gap: 4 }}>
        <button className="btn small" onClick={onPick}>Pilih…</button>
        {onClear && value && (
          <button className="btn small ghost" onClick={onClear} title="Kosongkan">✕</button>
        )}
      </div>
    </div>
  );
}

export function SourceCard({ side, value, onChange, connections, onManageConnections }: Props) {
  const [dbs, setDbs] = useState<string[]>([]);
  const [dbError, setDbError] = useState<string | null>(null);
  const [loadingDbs, setLoadingDbs] = useState(false);
  const set = (patch: Partial<SideState>) => onChange({ ...value, ...patch });

  const loadDbs = async (id: string) => {
    if (!id) {
      setDbs([]);
      return;
    }
    setLoadingDbs(true);
    setDbError(null);
    try {
      setDbs(await api.listDatabases(id));
    } catch (e) {
      setDbs([]);
      setDbError((e as Error).message);
    } finally {
      setLoadingDbs(false);
    }
  };

  useEffect(() => {
    if (value.mode === "live" && value.connectionId) loadDbs(value.connectionId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value.mode, value.connectionId]);

  const role = side === "a" ? "Target — akan diubah oleh script" : "Acuan — kondisi yang diinginkan";
  const conn = connections.find((c) => c.id === value.connectionId);

  return (
    <div className={`source ${side}`}>
      <div className="source-head">
        <span className="side-tag">{side.toUpperCase()}</span>
        <span className="side-role">{role}</span>
        <div className="segmented">
          <button className={value.mode === "file" ? "on" : ""} onClick={() => set({ mode: "file" })}>
            File dump
          </button>
          <button className={value.mode === "live" ? "on" : ""} onClick={() => set({ mode: "live" })}>
            Database
          </button>
        </div>
      </div>

      {value.mode === "file" ? (
        <>
          <PathRow
            label="Schema/dump"
            value={value.schemaPath}
            placeholder="File .sql (schema, atau schema + data)"
            onPick={async () => {
              const p = await pickSqlFile();
              if (p) set({ schemaPath: p });
            }}
          />
          <PathRow
            label="Data"
            value={value.dataPath}
            placeholder="Opsional: file data terpisah"
            onPick={async () => {
              const p = await pickSqlFile();
              if (p) set({ dataPath: p });
            }}
            onClear={() => set({ dataPath: "" })}
          />
        </>
      ) : (
        <>
          <div className="path-row">
            <label>Koneksi</label>
            <select
              className="select"
              value={value.connectionId}
              onChange={(e) => set({ connectionId: e.target.value, database: "" })}
            >
              <option value="">— pilih koneksi —</option>
              {connections.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name || `${c.user}@${c.host}`}
                </option>
              ))}
            </select>
            <button className="btn small" onClick={onManageConnections}>Kelola…</button>
          </div>
          <div className="path-row">
            <label>Database</label>
            <select
              className="select"
              value={value.database}
              disabled={!value.connectionId || loadingDbs}
              onChange={(e) => set({ database: e.target.value })}
              title={dbError ?? undefined}
            >
              <option value="">
                {loadingDbs ? "memuat…" : dbError ? "gagal memuat (lihat tooltip)" : "— pilih database —"}
              </option>
              {dbs.map((d) => (
                <option key={d} value={d}>{d}</option>
              ))}
            </select>
            <button
              className="btn small ghost"
              disabled={!value.connectionId}
              onClick={() => loadDbs(value.connectionId)}
              title={conn ? `${conn.user}@${conn.host}:${conn.port}` : "Muat ulang"}
            >
              ↻
            </button>
          </div>
        </>
      )}
    </div>
  );
}
