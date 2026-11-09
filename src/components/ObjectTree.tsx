import { useMemo, useState } from "react";
import type { Status, TableSummary } from "../lib/types";

export type Filter = "all" | "diff" | "onlyInA" | "onlyInB" | "same";

export function hasDataDiff(t: TableSummary): boolean {
  return !!t.data && (t.data.changed > 0 || t.data.onlyA > 0 || t.data.onlyB > 0);
}

export function isDifferent(t: TableSummary): boolean {
  return t.status !== "same" || hasDataDiff(t) || !!t.dataError;
}

export function scriptKey(t: TableSummary): string {
  return (t.nameB ?? t.nameA ?? t.name).toLowerCase();
}

const glyph: Record<Status, string> = { same: "=", modified: "~", onlyInA: "−", onlyInB: "+" };
const glyphTitle: Record<Status, string> = {
  same: "Schema sama",
  modified: "Schema berbeda",
  onlyInA: "Hanya di A (tidak ada di B)",
  onlyInB: "Hanya di B (akan dibuat di A)",
};

interface Props {
  tables: TableSummary[];
  selected: string | null;
  onSelect: (name: string) => void;
  checked: Set<string>;
  onCheck: (keys: string[], on: boolean) => void;
}

export function ObjectTree({ tables, selected, onSelect, checked, onCheck }: Props) {
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("diff");

  const counts = useMemo(
    () => ({
      all: tables.length,
      diff: tables.filter(isDifferent).length,
      onlyInA: tables.filter((t) => t.status === "onlyInA").length,
      onlyInB: tables.filter((t) => t.status === "onlyInB").length,
      same: tables.filter((t) => !isDifferent(t)).length,
    }),
    [tables],
  );

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return tables.filter((t) => {
      if (needle && !t.name.toLowerCase().includes(needle)) return false;
      switch (filter) {
        case "diff":
          return isDifferent(t);
        case "same":
          return !isDifferent(t);
        case "onlyInA":
        case "onlyInB":
          return t.status === filter;
        default:
          return true;
      }
    });
  }, [tables, q, filter]);

  const shownKeys = shown.filter(isDifferent).map(scriptKey);
  const allChecked = shownKeys.length > 0 && shownKeys.every((k) => checked.has(k));

  const chip = (f: Filter, label: string) => (
    <button className={`chip ${filter === f ? "on" : ""}`} onClick={() => setFilter(f)}>
      {label}
      <b>{counts[f]}</b>
    </button>
  );

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <input className="input" placeholder="Cari tabel…" value={q} onChange={(e) => setQ(e.target.value)} />
        <div className="chips">
          {chip("diff", "Berbeda")}
          {chip("all", "Semua")}
          {chip("onlyInB", "Hanya B")}
          {chip("onlyInA", "Hanya A")}
          {chip("same", "Sama")}
        </div>
        <label className="tree-select">
          <span>
            <input
              type="checkbox"
              checked={allChecked}
              disabled={shownKeys.length === 0}
              onChange={(e) => onCheck(shownKeys, e.target.checked)}
            />{" "}
            Ikutkan semua ke script
          </span>
          <span>{checked.size} dipilih</span>
        </label>
      </div>
      <div className="tree" role="listbox" aria-label="Daftar tabel">
        {shown.length === 0 && <div className="note" style={{ padding: "8px 12px" }}>Tidak ada tabel.</div>}
        {shown.map((t) => {
          const key = scriptKey(t);
          const diff = isDifferent(t);
          const d = t.data;
          return (
            <div
              key={t.name}
              className={`tree-row ${selected === t.name ? "sel" : ""}`}
              onClick={() => onSelect(t.name)}
              role="option"
              aria-selected={selected === t.name}
            >
              <input
                type="checkbox"
                checked={checked.has(key)}
                disabled={!diff}
                onClick={(e) => e.stopPropagation()}
                onChange={(e) => onCheck([key], e.target.checked)}
                title="Ikutkan ke script"
              />
              <span className={`glyph ${t.status}`} title={glyphTitle[t.status]}>{glyph[t.status]}</span>
              <span className="name" title={t.name}>{t.name}</span>
              <span className="meta">
                {t.status === "modified" && <span className="pill neutral" title="Jumlah perubahan schema">{t.changes.length}</span>}
                {t.dataError && <span className="pill del" title={t.dataError}>!</span>}
                {d && d.changed > 0 && <span className="pill mod" title="Baris berubah">~{d.changed}</span>}
                {d && d.onlyB > 0 && <span className="pill add" title="Baris hanya di B">+{d.onlyB}</span>}
                {d && d.onlyA > 0 && <span className="pill del" title="Baris hanya di A">−{d.onlyA}</span>}
              </span>
            </div>
          );
        })}
      </div>
    </aside>
  );
}
