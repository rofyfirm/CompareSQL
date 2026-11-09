import { useState } from "react";
import { api } from "../lib/api";
import type { ExecReport, ProgressEvent } from "../lib/types";

export function ProgressModal({ p, onCancel, title }: { p: ProgressEvent | null; onCancel: () => void; title: string }) {
  const pct = p && p.total > 0 ? Math.min(100, (p.done / p.total) * 100) : null;
  return (
    <div className="modal-back">
      <div className="modal" role="dialog" aria-label={title} style={{ width: 440 }}>
        <div className="modal-head">{title}</div>
        <div className="modal-body">
          <div>{p?.phase ?? "Menyiapkan…"}</div>
          <div className="muted mono" style={{ fontSize: 12, minHeight: 18 }}>
            {p?.detail}
            {p && p.total === 0 && p.done > 0 && ` · ${p.done.toLocaleString("id")} baris`}
          </div>
          <div className={`progress-bar ${pct === null ? "indet" : ""}`}>
            <div style={pct === null ? undefined : { width: `${pct}%` }} />
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn" onClick={onCancel}>Batal</button>
        </div>
      </div>
    </div>
  );
}

interface ExecProps {
  database: string;
  label: string;
  statements: number;
  onClose: () => void;
  onDone: () => void;
  progress: ProgressEvent | null;
}

export function ExecuteDialog({ database, label, statements, onClose, onDone, progress }: ExecProps) {
  const [typed, setTyped] = useState("");
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<ExecReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      setReport(await api.executeScript(typed));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRunning(false);
    }
  };

  const pct = progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;

  return (
    <div className="modal-back">
      <div className="modal" role="dialog" aria-label="Jalankan script">
        <div className="modal-head">Jalankan script di A</div>
        <div className="modal-body">
          {!report && (
            <>
              <div className="warn">
                Script akan mengubah database <b className="mono">{database}</b> ({label}): {statements.toLocaleString("id")}{" "}
                statement. DDL MySQL melakukan commit otomatis dan tidak bisa di-rollback. Pastikan ada backup.
              </div>
              <p className="note">
                Data diambil saat perbandingan. Bila A berubah sejak itu, bandingkan ulang dulu. DML dijalankan dalam
                transaksi per kelompok; eksekusi berhenti di error pertama.
              </p>
              <label className="check" style={{ display: "block" }}>
                Ketik <b className="mono">{database}</b> untuk konfirmasi:
                <input
                  className="input mono"
                  style={{ width: "100%", marginTop: 6 }}
                  autoFocus
                  value={typed}
                  disabled={running}
                  onChange={(e) => setTyped(e.target.value)}
                />
              </label>
              {running && (
                <>
                  <div className="progress-bar"><div style={{ width: `${pct}%` }} /></div>
                  <div className="muted">
                    {progress ? `${progress.done.toLocaleString("id")} / ${progress.total.toLocaleString("id")} statement` : "menghubungkan…"}
                  </div>
                </>
              )}
              {error && <div className="error" style={{ marginTop: 10 }}>{error}</div>}
            </>
          )}
          {report && !report.error && (
            <div>
              <h3 style={{ margin: "4px 0", color: "var(--add)" }}>Selesai</h3>
              <p>
                {report.executed.toLocaleString("id")} dari {report.total.toLocaleString("id")} statement berhasil dalam{" "}
                {(report.elapsedMs / 1000).toFixed(1)} detik.
              </p>
              <p className="note">Bandingkan ulang untuk memastikan A sudah sama dengan B.</p>
            </div>
          )}
          {report?.error && (
            <div>
              <div className="error">
                Gagal di statement #{report.error.index + 1}: {report.error.message}
                {report.error.rolledBack && "\nTransaksi DML yang sedang berjalan sudah di-rollback."}
              </div>
              <p className="note">{report.executed.toLocaleString("id")} statement sebelumnya sudah dijalankan.</p>
              <pre className="code selectable" style={{ maxHeight: 160, padding: 10 }}>{report.error.statement}</pre>
            </div>
          )}
        </div>
        <div className="modal-foot">
          {!report ? (
            <>
              <button className="btn" onClick={onClose} disabled={running}>Batal</button>
              <button className="btn danger" disabled={typed !== database || running} onClick={run}>
                {running ? "Menjalankan…" : "Jalankan"}
              </button>
            </>
          ) : (
            <>
              <button className="btn" onClick={onClose}>Tutup</button>
              <button className="btn primary" onClick={onDone}>Bandingkan ulang</button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
