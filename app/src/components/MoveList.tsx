import { useEffect, useRef } from "react";
import { CLASS_INFO } from "../lib/classes";
import type { MoveCell, MoveRow } from "../lib/rows";

export interface MoveListProps {
  rows: MoveRow[];
  selectedPly: number;
  onSelect: (ply: number) => void;
}

function Cell({
  cell,
  selected,
  onSelect,
}: {
  cell: MoveCell | null;
  selected: boolean;
  onSelect: (ply: number) => void;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (selected) ref.current?.scrollIntoView?.({ block: "nearest" });
  }, [selected]);

  if (!cell) return <span className="move move--empty">…</span>;
  const info = cell.review ? CLASS_INFO[cell.review.class] : null;
  const unsettled = Boolean(info && cell.provisional);
  return (
    <button
      ref={ref}
      type="button"
      className={`move${selected ? " move--selected" : ""}`}
      aria-current={selected ? "true" : undefined}
      aria-label={
        info ? `${cell.san}, ${info.label}${unsettled ? ", provisional" : ""}` : cell.san
      }
      onClick={() => onSelect(cell.ply)}
    >
      {cell.san}
      {info && (
        <span
          className={`move__mark${unsettled ? " move__mark--provisional" : ""}`}
          style={{ color: info.color }}
          aria-hidden="true"
        >
          {info.symbol}
        </span>
      )}
    </button>
  );
}

/** The game's moves in pairs, each marked with how good it was. */
export function MoveList({ rows, selectedPly, onSelect }: MoveListProps) {
  return (
    <ol className="move-list" aria-label="Moves">
      {rows.map((row) => (
        <li key={`${row.number}-${row.white?.ply ?? row.black?.ply}`} className="move-row">
          <span className="move-row__number">{row.number}.</span>
          <Cell cell={row.white} selected={row.white?.ply === selectedPly} onSelect={onSelect} />
          <Cell cell={row.black} selected={row.black?.ply === selectedPly} onSelect={onSelect} />
        </li>
      ))}
    </ol>
  );
}
