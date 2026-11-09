import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { CellText, DataDetail, TableDetail, TableSummary } from "../lib/types";
import { hasDataDiff } from "./ObjectTree";

const categoryLabel: Record<string, string> = {
  column: "Kolom",
  primary_key: "Primary key",
  index: "Index",
  foreign_key: "Foreign key",
  check: "Check",
  option: "Opsi tabel",
};

const actionPill: Record<string, [string, string]> = {
  added: ["add", "tambah"],
  removed: ["del", "hapus"],
  modified: ["mod", "ubah"],
  moved: ["neutral", "pindah"],
};

function SchemaPane({ summary, detail }: { summary: TableSummary; detail: TableDetail }) {
  return (
    <>
      {summary.status === "onlyInB" && (
        <div className="warn">Tabel hanya ada di B. Script akan membuatnya di A.</div>
      )}
      {summary.status === "onlyInA" && (
        <div className="warn">
          Tabel hanya ada di A. Script tidak menghapusnya kecuali opsi "Drop tabel yang hanya di A" aktif.
        </div>
      )}
      {summary.changes.length > 0 && (
        <>
          <div className="section-title">Perubahan untuk A ({summary.changes.length})</div>
          <div className="card">
            <table className="changes selectable">
              <tbody>
                {summary.changes.map((c, i) => {
                  const [cls, label] = actionPill[c.action] ?? ["neutral", c.action];
                  return (
                    <tr key={i}>
                      <td className="cat">
                        <span className={`pill ${cls}`}>{label}</span> {categoryLabel[c.category] ?? c.category}
                      </td>
                      <td className="obj">{c.name}</td>
                      <td className="det">{c.detail}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </>
      )}
      {summary.status === "same" && <p className="note">Schema sama setelah normalisasi.</p>}
      <div className="section-title">Definisi (kanonik)</div>
      <div className="card sbs selectable">
        <div className="sbs-head a">A</div>
        <div className="sbs-head b">B</div>
        {detail.lines.map((l, i) => (
          <Line key={i} kind={l.kind} a={l.a} b={l.b} />
        ))}
      </div>
    </>
  );
}

function Line({ kind, a, b }: { kind: string; a: string | null; b: string | null }) {
  return (
    <>
      <div className={`sbs-line left k-${kind} ${a === null ? "blank" : ""}`}>{a ?? ""}</div>
      <div className={`sbs-line right k-${kind} ${b === null ? "blank" : ""}`}>{b ?? ""}</div>
    </>
  );
}

function Cell({ v, diff }: { v: CellText; diff?: boolean }) {
  const cls = [v === null ? "null" : "", diff ? "diff" : ""].join(" ");
  return <td className={cls} title={v ?? "NULL"}>{v ?? "NULL"}</td>;
}

function Head({ d, withSide }: { d: DataDetail; withSide?: boolean }) {
  return (
    <thead>
      <tr>
        {withSide && <th />}
        {d.columns.map((c, i) => (
          <th
            key={c}
            className={[d.keyColumns.includes(i) ? "key" : "", d.missingInA.includes(i) ? "missing" : ""].join(" ")}
            title={d.missingInA.includes(i) ? "Kolom baru (belum ada di A)" : undefined}
          >
            {c}
          </th>
        ))}
      </tr>
    </thead>
  );
}

type DataTab = "changed" | "onlyB" | "onlyA";

function DataPane({ summary, d }: { summary: TableSummary; d: DataDetail }) {
  const s = summary.data!;
  const first: DataTab = s.changed > 0 ? "changed" : s.onlyB > 0 ? "onlyB" : "onlyA";
  const [tab, setTab] = useState<DataTab>(first);
  useEffect(() => setTab(first), [summary.name, first]);

  const total = { changed: d.changedTotal, onlyB: d.onlyBTotal, onlyA: d.onlyATotal }[tab];
  const shownN = { changed: d.changed.length, onlyB: d.onlyB.length, onlyA: d.onlyA.length }[tab];

  return (
    <>
      <div className="stats">
        <div className="card stat"><div className="v">{s.rowsA.toLocaleString("id")}</div><div className="l">baris di A</div></div>
        <div className="card stat"><div className="v">{s.rowsB.toLocaleString("id")}</div><div className="l">baris di B</div></div>
        <div className="card stat mod"><div className="v">{s.changed.toLocaleString("id")}</div><div className="l">berubah (UPDATE)</div></div>
        <div className="card stat add"><div className="v">{s.onlyB.toLocaleString("id")}</div><div className="l">hanya di B (INSERT)</div></div>
        <div className="card stat del"><div className="v">{s.onlyA.toLocaleString("id")}</div><div className="l">hanya di A (DELETE)</div></div>
      </div>
      {s.warnings.length > 0 && (
        <div className="warn" style={{ marginTop: 10 }}>
          <ul>{s.warnings.map((w) => <li key={w}>{w}</li>)}</ul>
        </div>
      )}
      <p className="note">
        Kunci perbandingan:{" "}
        <b>
          {d.keyMode === "fullRow"
            ? "seluruh baris (tabel tanpa key)"
            : d.keyColumns.map((k) => d.columns[k]).join(", ") + (d.keyMode === "uniqueKey" ? " (unique)" : " (primary key)")}
        </b>
        {d.missingInA.length > 0 && <> · kolom hijau belum ada di A (dibandingkan dengan nilai default B)</>}
      </p>

      {!hasDataDiff(summary) ? (
        <p className="note">Data sama.</p>
      ) : (
        <>
          <div className="segmented" style={{ marginBottom: 8 }}>
            <button className={tab === "changed" ? "on" : ""} onClick={() => setTab("changed")}>Berubah {s.changed}</button>
            <button className={tab === "onlyB" ? "on" : ""} onClick={() => setTab("onlyB")}>Hanya di B {s.onlyB}</button>
            <button className={tab === "onlyA" ? "on" : ""} onClick={() => setTab("onlyA")}>Hanya di A {s.onlyA}</button>
          </div>
          <div className="card grid-wrap">
            <table className="grid">
              <Head d={d} withSide={tab === "changed"} />
              <tbody>
                {tab === "changed" &&
                  d.changed.map((r, i) => (
                    <FragmentRows key={i} a={r.a} b={r.b} diff={r.diff} />
                  ))}
                {tab === "onlyB" &&
                  d.onlyB.map((r, i) => (
                    <tr key={i}>{r.map((v, j) => <Cell key={j} v={v} />)}</tr>
                  ))}
                {tab === "onlyA" &&
                  d.onlyA.map((r, i) => (
                    <tr key={i}>{r.map((v, j) => <Cell key={j} v={v} />)}</tr>
                  ))}
              </tbody>
            </table>
          </div>
          {total > shownN && (
            <p className="note">Menampilkan {shownN.toLocaleString("id")} dari {total.toLocaleString("id")} baris. Script tetap memuat semuanya.</p>
          )}
        </>
      )}
    </>
  );
}

function FragmentRows({ a, b, diff }: { a: CellText[]; b: CellText[]; diff: number[] }) {
  return (
    <>
      <tr className="row-a">
        <td className="side">A</td>
        {a.map((v, j) => <Cell key={j} v={v} diff={diff.includes(j)} />)}
      </tr>
      <tr className="row-b">
        <td className="side">B</td>
        {b.map((v, j) => <Cell key={j} v={v} diff={diff.includes(j)} />)}
      </tr>
    </>
  );
}

export function TableView({ summary, dataCompared, dataSkipped }: {
  summary: TableSummary;
  dataCompared: boolean;
  dataSkipped: string | null;
}) {
  const [detail, setDetail] = useState<TableDetail | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const defaultTab = summary.status !== "same" || !hasDataDiff(summary) ? "schema" : "data";
  const [tab, setTab] = useState<"schema" | "data">(defaultTab);

  useEffect(() => {
    let live = true;
    setDetail(null);
    setErr(null);
    setTab(defaultTab);
    api
      .tableDetail(summary.name)
      .then((d) => live && setDetail(d))
      .catch((e) => live && setErr((e as Error).message));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [summary.name]);

  return (
    <section className="content">
      <div className="content-head">
        <h2>{summary.name}</h2>
        {summary.nameA && summary.nameB && summary.nameA !== summary.nameB && (
          <span className="muted">A: {summary.nameA} · B: {summary.nameB}</span>
        )}
        <div className="tabs">
          <button className={`tab ${tab === "schema" ? "on" : ""}`} onClick={() => setTab("schema")}>
            Schema {summary.changes.length > 0 && `(${summary.changes.length})`}
          </button>
          <button className={`tab ${tab === "data" ? "on" : ""}`} onClick={() => setTab("data")}>
            Data {hasDataDiff(summary) && "•"}
          </button>
        </div>
      </div>
      <div className="content-body">
        {err && <div className="error">{err}</div>}
        {!detail && !err && <p className="note">Memuat…</p>}
        {detail && tab === "schema" && <SchemaPane summary={summary} detail={detail} />}
        {detail && tab === "data" && (
          <>
            {summary.dataError && <div className="error">Data tabel ini gagal dibandingkan: {summary.dataError}</div>}
            {!dataCompared && <p className="note">{dataSkipped ?? "Perbandingan data dimatikan."}</p>}
            {dataCompared && !detail.data && !summary.dataError && (
              <p className="note">Tabel hanya ada di A: tidak ada data acuan di B.</p>
            )}
            {detail.data && summary.data && <DataPane summary={summary} d={detail.data} />}
          </>
        )}
      </div>
    </section>
  );
}
