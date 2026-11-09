import { useCallback, useEffect, useRef, useState } from "react";
import { api, onProgress } from "./lib/api";
import {
  defaultCompareOptions,
  defaultScriptOptions,
  type CompareOptions,
  type CompareSummary,
  type ConnectionProfile,
  type ProgressEvent,
  type ScriptOptions,
} from "./lib/types";
import { emptySide, sideReady, SourceCard, toInput, type SideState } from "./components/SourceCard";
import { ConnectionsDialog } from "./components/ConnectionsDialog";
import { OptionsPopover } from "./components/OptionsPopover";
import { isDifferent, ObjectTree, scriptKey } from "./components/ObjectTree";
import { TableView } from "./components/TableView";
import { ScriptView } from "./components/ScriptView";
import { ExecuteDialog, ProgressModal } from "./components/Dialogs";

// Preferensi per pengguna (path terakhir, opsi). Aman bila storage tidak tersedia.
function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? { ...fallback, ...JSON.parse(raw) } : fallback;
  } catch {
    return fallback;
  }
}
function store(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* abaikan */
  }
}

export default function App() {
  const [a, setA] = useState<SideState>(() => load("selisih.a", emptySide));
  const [b, setB] = useState<SideState>(() => load("selisih.b", emptySide));
  const [options, setOptions] = useState<CompareOptions>(() => load("selisih.opts", defaultCompareOptions));
  const [compareData, setCompareData] = useState<boolean>(() => load("selisih.data", { v: true }).v);
  const [scriptOpts, setScriptOpts] = useState<ScriptOptions>(defaultScriptOptions);

  const [connections, setConnections] = useState<ConnectionProfile[]>([]);
  const [showConn, setShowConn] = useState(false);
  const [showOpts, setShowOpts] = useState(false);

  const [summary, setSummary] = useState<CompareSummary | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [view, setView] = useState<"tables" | "script">("tables");

  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [execStatements, setExecStatements] = useState<number | null>(null);

  useEffect(() => store("selisih.a", a), [a]);
  useEffect(() => store("selisih.b", b), [b]);
  useEffect(() => store("selisih.opts", options), [options]);
  useEffect(() => store("selisih.data", { v: compareData }), [compareData]);

  const refreshConnections = useCallback(() => {
    api.listConnections().then(setConnections).catch((e) => setError((e as Error).message));
  }, []);
  useEffect(refreshConnections, [refreshConnections]);

  useEffect(() => {
    let un: (() => void) | undefined;
    onProgress(setProgress).then((u) => (un = u));
    return () => un?.();
  }, []);

  const ready = sideReady(a) && sideReady(b);

  const runCompare = useCallback(async () => {
    if (!sideReady(a) || !sideReady(b)) return;
    setBusy(true);
    setProgress(null);
    setError(null);
    try {
      const s = await api.compare({ a: toInput(a), b: toInput(b), compareData, options });
      setSummary(s);
      const diffs = s.tables.filter(isDifferent);
      setChecked(new Set(diffs.map(scriptKey)));
      setSelected((cur) => (cur && s.tables.some((t) => t.name === cur) ? cur : diffs[0]?.name ?? s.tables[0]?.name ?? null));
      setView("tables");
    } catch (e) {
      const msg = (e as Error).message;
      if (!/dibatalkan/i.test(msg)) setError(msg);
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }, [a, b, compareData, options]);

  // Cmd+Enter = bandingkan
  const compareRef = useRef(runCompare);
  compareRef.current = runCompare;
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter") compareRef.current();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  const onCheck = (keys: string[], on: boolean) => {
    setChecked((prev) => {
      const next = new Set(prev);
      keys.forEach((k) => (on ? next.add(k) : next.delete(k)));
      return next;
    });
  };

  // Pilihan tabel di tree selalu ikut ke opsi script selama panel script terbuka.
  useEffect(() => {
    if (view === "script") setScriptOpts((o) => ({ ...o, tables: [...checked] }));
  }, [checked, view]);

  const openScript = () => {
    setScriptOpts((o) => ({ ...o, tables: [...checked], includeData: !!summary?.dataCompared }));
    setView("script");
  };

  const swap = () => {
    setA(b);
    setB(a);
    setSummary(null);
  };

  const sel = summary?.tables.find((t) => t.name === selected) ?? null;
  const nDiff = summary ? summary.tables.filter(isDifferent).length : 0;

  return (
    <div className="app">
      <header className="toolbar" style={{ position: "relative" }}>
        <SourceCard side="a" value={a} onChange={setA} connections={connections} onManageConnections={() => setShowConn(true)} />
        <button className="btn icon swap" title="Tukar A dan B" onClick={swap}>⇄</button>
        <SourceCard side="b" value={b} onChange={setB} connections={connections} onManageConnections={() => setShowConn(true)} />
        <div className="toolbar-actions">
          <button className="btn primary" disabled={!ready || busy} onClick={runCompare} title="⌘↩">
            Bandingkan
          </button>
          <button className="btn" onClick={() => setShowOpts((v) => !v)}>Opsi…</button>
          <button className="btn" disabled={!summary || checked.size === 0} onClick={openScript}>
            Script sinkron…
          </button>
        </div>
        {showOpts && (
          <OptionsPopover
            options={options}
            compareData={compareData}
            onChange={setOptions}
            onCompareData={setCompareData}
            onClose={() => setShowOpts(false)}
          />
        )}
      </header>

      <div className="main">
        {summary ? (
          <ObjectTree tables={summary.tables} selected={selected} onSelect={(n) => { setSelected(n); setView("tables"); }} checked={checked} onCheck={onCheck} />
        ) : (
          <aside className="sidebar" />
        )}

        {error && (
          <div className="error toast" role="alert">
            {error}
            <div style={{ textAlign: "right", marginTop: 6 }}>
              <button className="btn small" onClick={() => setError(null)}>Tutup</button>
            </div>
          </div>
        )}

        {view === "script" && summary ? (
          <ScriptView
            options={scriptOpts}
            onOptions={setScriptOpts}
            selectedCount={checked.size}
            canExecute={summary.aIsLive}
            targetDb={summary.aDatabase}
            onExecute={setExecStatements}
            onBack={() => setView("tables")}
            onError={setError}
          />
        ) : sel && summary ? (
          <TableView summary={sel} dataCompared={summary.dataCompared} dataSkipped={summary.dataSkipped} />
        ) : (
          <section className="content">
            <div className="empty-state">
              {summary ? (
                <div>
                  <h3>Tidak ada tabel</h3>
                  <p>Kedua sisi tidak berisi CREATE TABLE.</p>
                </div>
              ) : (
                <div>
                  <h3>Bandingkan dua database MySQL / MariaDB</h3>
                  <ol>
                    <li><b>A</b> adalah target yang akan diubah. <b>B</b> adalah acuan.</li>
                    <li>Pilih file dump (mysqldump / mariadb-dump) atau koneksi database untuk tiap sisi.</li>
                    <li>Klik <b>Bandingkan</b> (⌘↩). Lihat beda schema dan data per tabel.</li>
                    <li>Buat <b>script sinkron</b>, tinjau, simpan, atau jalankan di A setelah konfirmasi.</li>
                  </ol>
                </div>
              )}
            </div>
          </section>
        )}
      </div>

      <footer className="statusbar">
        {summary ? (
          <>
            <span><b style={{ color: "var(--side-a)" }}>A</b> {summary.labelA}{summary.versionA && ` (${summary.versionA})`}</span>
            <span><b style={{ color: "var(--side-b)" }}>B</b> {summary.labelB}{summary.versionB && ` (${summary.versionB})`}</span>
            <span>{summary.tables.length} tabel · {nDiff} berbeda</span>
            {summary.dataSkipped && <span title={summary.dataSkipped}>data tidak dibandingkan</span>}
            {(summary.skippedA.length > 0 || summary.skippedB.length > 0) && (
              <span title={[...summary.skippedA, ...summary.skippedB].map((o) => `${o.kind} ${o.name}`).join("\n")}>
                {summary.skippedA.length + summary.skippedB.length} objek di luar scope (view/trigger/routine)
              </span>
            )}
            {summary.warnings.length > 0 && (
              <span title={summary.warnings.join("\n")} style={{ color: "var(--mod)" }}>
                {summary.warnings.length} peringatan
              </span>
            )}
            <span className="spacer" />
            <span>{(summary.elapsedMs / 1000).toFixed(2)} dtk</span>
          </>
        ) : (
          <span>Belum ada perbandingan</span>
        )}
      </footer>

      {showConn && (
        <ConnectionsDialog connections={connections} onClose={() => setShowConn(false)} onChanged={refreshConnections} />
      )}
      {busy && <ProgressModal title="Membandingkan" p={progress} onCancel={() => api.cancel()} />}
      {execStatements !== null && summary?.aDatabase && (
        <ExecuteDialog
          database={summary.aDatabase}
          label={summary.labelA}
          statements={execStatements}
          progress={progress}
          onClose={() => setExecStatements(null)}
          onDone={() => {
            setExecStatements(null);
            runCompare();
          }}
        />
      )}
    </div>
  );
}
