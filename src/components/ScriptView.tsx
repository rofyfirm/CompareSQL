import { useEffect, useMemo, useState } from "react";
import { api, pickSavePath } from "../lib/api";
import type { ScriptOptions, ScriptPreview } from "../lib/types";

interface Props {
  options: ScriptOptions;
  onOptions: (o: ScriptOptions) => void;
  selectedCount: number;
  canExecute: boolean;
  targetDb: string | null;
  onExecute: (statements: number) => void;
  onBack: () => void;
  onError: (msg: string) => void;
}

const KW = /^(\s*)(ALTER TABLE|CREATE TABLE|INSERT INTO|UPDATE|SET|ADD|MODIFY COLUMN|ADD COLUMN|ADD CONSTRAINT|ADD KEY|ADD UNIQUE KEY|ADD PRIMARY KEY)\b/;
const DANGER = /^(\s*)(DROP TABLE|DROP COLUMN|DROP INDEX|DROP FOREIGN KEY|DROP PRIMARY KEY|DROP CONSTRAINT|DELETE FROM|ALTER TABLE `[^`]+` DROP FOREIGN KEY)\b/;
const MAX_HL_LINES = 6000;

function Highlighted({ text }: { text: string }) {
  const lines = useMemo(() => text.split("\n"), [text]);
  return (
    <>
      {lines.map((l, i) => {
        if (i >= MAX_HL_LINES) return <span className="ln" key={i}>{l}</span>;
        if (l.startsWith("--")) return <span className="ln cm" key={i}>{l}</span>;
        const d = DANGER.exec(l);
        if (d) {
          return (
            <span className="ln" key={i}>
              {d[1]}<span className="del">{d[2]}</span>{l.slice(d[0].length)}
            </span>
          );
        }
        const m = KW.exec(l);
        if (m) {
          return (
            <span className="ln" key={i}>
              {m[1]}<span className="kw">{m[2]}</span>{l.slice(m[0].length)}
            </span>
          );
        }
        return <span className="ln" key={i}>{l}</span>;
      })}
    </>
  );
}

function fmtBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1 << 20) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1 << 20)).toFixed(1)} MB`;
}

export function ScriptView(p: Props) {
  const { options: o, onOptions } = p;
  const [preview, setPreview] = useState<ScriptPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  const regenerate = async (opts: ScriptOptions) => {
    setBusy(true);
    try {
      setPreview(await api.generateScript(opts));
    } catch (e) {
      p.onError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    // Debounce: opsi bisa berubah beberapa kali berturut-turut.
    const t = setTimeout(() => regenerate(o), 120);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [o]);

  const set = (patch: Partial<ScriptOptions>) => onOptions({ ...o, ...patch });

  const save = async () => {
    const path = await pickSavePath("selisih-sync.sql");
    if (!path) return;
    try {
      await api.saveScript(path);
    } catch (e) {
      p.onError((e as Error).message);
    }
  };

  const copy = async () => {
    if (!preview) return;
    await navigator.clipboard.writeText(preview.preview);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  const box = (key: keyof ScriptOptions, label: string, hint?: string) => (
    <label className="check">
      <input type="checkbox" checked={o[key] as boolean} onChange={(e) => set({ [key]: e.target.checked })} />
      <span>
        {label}
        {hint && <small>{hint}</small>}
      </span>
    </label>
  );

  return (
    <section className="content">
      <div className="content-head">
        <button className="btn small ghost" onClick={p.onBack}>← Hasil</button>
        <h2 style={{ fontFamily: "inherit" }}>Script sinkronisasi A ← B</h2>
        <span className="muted">{p.selectedCount} tabel dipilih</span>
      </div>
      <div className="content-body">
        <div className="script-layout">
          <div className="script-opts">
            <div className="section-title">Isi script</div>
            {box("includeSchema", "Perubahan schema", "CREATE / ALTER / FK")}
            {box("includeData", "Perubahan data", "INSERT / UPDATE / DELETE")}
            <div className="section-title">Operasi berisiko</div>
            {box("dropTables", "Drop tabel yang hanya di A")}
            {box("dropColumns", "Drop kolom yang hanya di A", "data kolom itu hilang")}
            {box("deleteRows", "Hapus baris yang hanya di A")}
            {box("disableFkChecks", "Nonaktifkan FOREIGN_KEY_CHECKS", "hanya bila ada siklus FK")}
            <div className="section-title">Batch</div>
            <label className="check">
              <span style={{ flex: 1 }}>Baris per INSERT</span>
              <input
                className="input"
                style={{ width: 80 }}
                type="number"
                min={1}
                value={o.batchRows}
                onChange={(e) => set({ batchRows: Math.max(1, Number(e.target.value) || 1) })}
              />
            </label>
          </div>
          <div className="script-main">
            <div className="script-bar">
              {preview && (
                <span className="muted">
                  {preview.statements.toLocaleString("id")} statement · {fmtBytes(preview.totalBytes)}
                </span>
              )}
              {busy && <span className="muted">membuat…</span>}
              <span className="spacer" />
              <button className="btn" disabled={!preview || preview.truncated} onClick={copy} title={preview?.truncated ? "Script terlalu besar; simpan ke file" : ""}>
                {copied ? "Tersalin" : "Salin"}
              </button>
              <button className="btn" disabled={!preview} onClick={save}>Simpan .sql…</button>
              <button
                className="btn danger"
                disabled={!preview || !p.canExecute || busy}
                onClick={() => preview && p.onExecute(preview.statements)}
                title={p.canExecute ? `Jalankan di database ${p.targetDb}` : "Hanya bisa bila A adalah database live"}
              >
                Jalankan di A…
              </button>
            </div>
            {preview && preview.warnings.length > 0 && (
              <div className="warn">
                <b>Perhatikan sebelum menjalankan:</b>
                <ul>{preview.warnings.map((w, i) => <li key={i}>{w}</li>)}</ul>
              </div>
            )}
            {preview?.truncated && (
              <div className="warn">Pratinjau dipotong ({fmtBytes(preview.preview.length)}). Simpan ke file untuk melihat semuanya.</div>
            )}
            <pre className="code selectable" style={{ flex: 1 }}>
              {preview ? <Highlighted text={preview.preview} /> : <span className="ln cm">-- membuat script…</span>}
            </pre>
          </div>
        </div>
      </div>
    </section>
  );
}
