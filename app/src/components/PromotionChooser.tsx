const PROMOTIONS = [
  { label: "Queen", piece: "q" },
  { label: "Rook", piece: "r" },
  { label: "Bishop", piece: "b" },
  { label: "Knight", piece: "n" },
];

export interface PromotionChooserProps {
  /** Called with the UCI letter of the chosen piece: q, r, b or n. */
  onChoose: (piece: string) => void;
  onCancel: () => void;
}

/** Asks which piece a pawn that reached the last rank becomes. */
export function PromotionChooser({ onChoose, onCancel }: PromotionChooserProps) {
  return (
    <div role="group" aria-label="Promote to" className="record__promotion">
      {PROMOTIONS.map(({ label, piece }) => (
        <button key={piece} type="button" onClick={() => onChoose(piece)}>
          {label}
        </button>
      ))}
      <button type="button" onClick={onCancel}>
        Cancel promotion
      </button>
    </div>
  );
}
