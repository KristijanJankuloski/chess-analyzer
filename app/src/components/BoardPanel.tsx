import { useEffect, useState } from "react";
import { Chessboard, type Arrow, type ChessboardOptions } from "react-chessboard";
import type { MoveClass } from "../generated/MoveClass";
import type { SquarePair } from "../lib/board";
import { CLASS_INFO } from "../lib/classes";

export interface BoardPanelProps {
  fen: string;
  orientation: "white" | "black";
  lastMove?: SquarePair | null;
  bestArrow?: SquarePair | null;
  badge?: { square: string; cls: MoveClass } | null;
  /**
   * Makes the board playable, by dragging or by clicking the piece and then its destination.
   * Return true to accept the move, false to refuse it (a dragged piece snaps back).
   */
  onMove?: (from: string, to: string) => boolean;
}

const LAST_MOVE_STYLE = { backgroundColor: "rgba(255, 213, 0, 0.42)" };
const SELECTED_STYLE = { boxShadow: "inset 0 0 0 4px rgba(129, 182, 76, 0.95)" };
const BEST_ARROW_COLOR = "rgba(129, 182, 76, 0.92)";

export function BoardPanel({
  fen,
  orientation,
  lastMove,
  bestArrow,
  badge,
  onMove,
}: BoardPanelProps) {
  const [selected, setSelected] = useState<string | null>(null);
  // A new position means the old selection no longer means anything.
  useEffect(() => setSelected(null), [fen]);

  const squareStyles: Record<string, React.CSSProperties> = {};
  if (lastMove) {
    squareStyles[lastMove.from] = LAST_MOVE_STYLE;
    squareStyles[lastMove.to] = LAST_MOVE_STYLE;
  }
  if (selected) squareStyles[selected] = { ...squareStyles[selected], ...SELECTED_STYLE };

  const arrows: Arrow[] = bestArrow
    ? [{ startSquare: bestArrow.from, endSquare: bestArrow.to, color: BEST_ARROW_COLOR }]
    : [];

  const options: ChessboardOptions = {
    id: "review-board",
    position: fen,
    boardOrientation: orientation,
    squareStyles,
    arrows,
    allowDrawingArrows: false,
    allowDragging: Boolean(onMove),
    showAnimations: true,
    animationDurationInMs: 150,
    darkSquareStyle: { backgroundColor: "#b58863" },
    lightSquareStyle: { backgroundColor: "#f0d9b5" },
    onPieceDrop: ({ sourceSquare, targetSquare }) => {
      setSelected(null);
      return onMove && targetSquare ? onMove(sourceSquare, targetSquare) : false;
    },
    onSquareClick: ({ piece, square }) => {
      if (!onMove) return;
      if (selected && selected !== square && onMove(selected, square)) {
        setSelected(null);
        return;
      }
      // Either nothing was selected, the move was refused, or the same square was clicked again.
      setSelected(piece && square !== selected ? square : null);
    },
    squareRenderer: ({ square, children }) => (
      <div className="square" data-square={square}>
        {children}
        {badge && badge.square === square ? (
          <span
            className="class-badge"
            style={{ backgroundColor: CLASS_INFO[badge.cls].color }}
            title={CLASS_INFO[badge.cls].label}
          >
            {CLASS_INFO[badge.cls].symbol}
          </span>
        ) : null}
      </div>
    ),
  };

  return (
    <div className="board-panel">
      <Chessboard options={options} />
    </div>
  );
}
