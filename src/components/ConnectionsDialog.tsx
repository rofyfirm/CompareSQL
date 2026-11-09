import { useState } from "react";
import { api } from "../lib/api";
import type { ConnectionProfile } from "../lib/types";

const blank: ConnectionProfile = {
  id: "",
  name: "",
  host: "127.0.0.1",
  port: 3306,
  user: "root",
  useTls: false,
  acceptInvalidCerts: false,
};

interface Props {
  connections: ConnectionProfile[];
  onClose: () => void;
  onChanged: () => void;
}

export function ConnectionsDialog({ connections, onClose, onChanged }: Props) {
  const [sel, setSel] = useState<ConnectionProfile>(connections[0] ?? blank);
  const [password, setPassword] = useState("");
  const [pwTouched, setPwTouched] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const set = (patch: Partial<ConnectionProfile>) => setSel({ ...sel, ...patch });
  const pick = (c: ConnectionProfile) => {
    setSel(c);
    setPassword("");
    setPwTouched(false);
    setMsg(null);
  };

  const test = async () => {
    setBusy(true);
    setMsg(null);
    try {
      const v = await api.testConnection(sel, pwTouched ? password : null);
      setMsg({ ok: true, text: `Terhubung. Server ${v}` });
    } catch (e) {
      setMsg({ ok: false, text: (e as Error).message });
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    try {
      const saved = await api.saveConnection(sel, pwTouched ? password : null);
      setSel(saved);
      setPassword("");
      setPwTouched(false);
      setMsg({ ok: true, text: "Tersimpan. Password ada di Keychain macOS." });
      onChanged();
    } catch (e) {
      setMsg({ ok: false, text: (e as Error).message });
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!sel.id) return;
    await api.deleteConnection(sel.id);
    onChanged();
    pick(blank);
  };

  return (
    <div className="modal-back" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide" role="dialog" aria-label="Koneksi database">
        <div className="modal-head">Koneksi database</div>
        <div className="modal-body">
          <div className="conn-layout">
            <div>
              <div className="conn-list">
                {connections.length === 0 && <div className="conn-item muted">Belum ada koneksi</div>}
                {connections.map((c) => (
                  <div key={c.id} className={`conn-item ${c.id === sel.id ? "sel" : ""}`} onClick={() => pick(c)}>
                    {c.name || "(tanpa nama)"}
                    <small>{c.user}@{c.host}:{c.port}</small>
                  </div>
                ))}
              </div>
              <button className="btn small" style={{ marginTop: 8 }} onClick={() => pick(blank)}>
                + Koneksi baru
              </button>
            </div>
            <div className="form-grid">
              <label>Nama</label>
              <input className="input" value={sel.name} placeholder="mis. Produksi klinik" onChange={(e) => set({ name: e.target.value })} />
              <label>Host</label>
              <input className="input" value={sel.host} onChange={(e) => set({ host: e.target.value })} />
              <label>Port</label>
              <input
                className="input"
                type="number"
                value={sel.port}
                onChange={(e) => set({ port: Number(e.target.value) || 3306 })}
              />
              <label>User</label>
              <input className="input" value={sel.user} onChange={(e) => set({ user: e.target.value })} />
              <label>Password</label>
              <input
                className="input"
                type="password"
                value={password}
                placeholder={sel.hasPassword ? "•••••• (tersimpan di Keychain)" : ""}
                onChange={(e) => {
                  setPassword(e.target.value);
                  setPwTouched(true);
                }}
              />
              <span />
              <label className="check" style={{ textAlign: "left" }}>
                <input type="checkbox" checked={sel.useTls} onChange={(e) => set({ useTls: e.target.checked })} />
                <span>Pakai TLS</span>
              </label>
              <span />
              <label className="check" style={{ textAlign: "left" }}>
                <input
                  type="checkbox"
                  checked={sel.acceptInvalidCerts}
                  disabled={!sel.useTls}
                  onChange={(e) => set({ acceptInvalidCerts: e.target.checked })}
                />
                <span>Terima sertifikat self-signed</span>
              </label>
            </div>
          </div>
          {msg && <div className={msg.ok ? "warn" : "error"} style={{ marginTop: 12 }}>{msg.text}</div>}
          <p className="note">
            Profil disimpan di folder konfigurasi aplikasi. Password tidak pernah ditulis ke file; hanya ke Keychain.
          </p>
        </div>
        <div className="modal-foot">
          {sel.id && (
            <button className="btn ghost" style={{ marginRight: "auto", color: "var(--del)" }} onClick={remove}>
              Hapus
            </button>
          )}
          <button className="btn" disabled={busy || !sel.host} onClick={test}>Tes koneksi</button>
          <button className="btn" onClick={onClose}>Tutup</button>
          <button className="btn primary" disabled={busy || !sel.host || !sel.user} onClick={save}>Simpan</button>
        </div>
      </div>
    </div>
  );
}
