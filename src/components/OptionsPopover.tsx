import type { CompareOptions } from "../lib/types";

interface Props {
  options: CompareOptions;
  compareData: boolean;
  onChange: (o: CompareOptions) => void;
  onCompareData: (v: boolean) => void;
  onClose: () => void;
}

const items: { key: keyof CompareOptions; label: string; hint: string }[] = [
  { key: "ignoreIntDisplayWidth", label: "Abaikan lebar int", hint: "int(11) = int (MySQL 8 vs MariaDB)" },
  { key: "ignoreAutoIncrement", label: "Abaikan nilai AUTO_INCREMENT", hint: "angka counter tabel" },
  { key: "ignoreTableNameCase", label: "Nama tabel tidak peka huruf", hint: "Pasien = pasien" },
  { key: "ignoreColumnOrder", label: "Abaikan urutan kolom", hint: "tidak buat MODIFY … AFTER" },
  { key: "ignoreComments", label: "Abaikan komentar", hint: "kolom, index, tabel" },
  { key: "ignoreCharsetCollation", label: "Abaikan charset & collation", hint: "" },
  { key: "ignoreEngine", label: "Abaikan engine", hint: "InnoDB vs MyISAM" },
  { key: "ignoreRowFormat", label: "Abaikan ROW_FORMAT", hint: "" },
];

export function OptionsPopover({ options, compareData, onChange, onCompareData, onClose }: Props) {
  return (
    <>
      <div style={{ position: "fixed", inset: 0, zIndex: 39 }} onMouseDown={onClose} />
      <div className="popover">
        <div className="section-title">Bandingkan</div>
        <label className="check">
          <input type="checkbox" checked={compareData} onChange={(e) => onCompareData(e.target.checked)} />
          <span>
            Data (berdasarkan primary key)
            <small>Matikan untuk schema saja; lebih cepat</small>
          </span>
        </label>
        <div className="section-title">Normalisasi</div>
        {items.map((it) => (
          <label className="check" key={it.key}>
            <input
              type="checkbox"
              checked={options[it.key]}
              onChange={(e) => onChange({ ...options, [it.key]: e.target.checked })}
            />
            <span>
              {it.label}
              {it.hint && <small>{it.hint}</small>}
            </span>
          </label>
        ))}
      </div>
    </>
  );
}
