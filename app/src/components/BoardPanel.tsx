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
  /** Makes the board playable. Return true to accept the move, false to snap the piece back. */
  onMove?: (from: string, to: string) => boolean;
}

const LAST_MOVE_STYLE = { backgroundColor: "rgba(255, 213, 0, 0.42)" };
const BEST_ARROW_COLOR = "rgba(129, 182, 76, 0.92)";

export function BoardPanel({
  fen,
  orientation,
  lastMove,
  bestArrow,
  badge,
  onMove,
}: BoardPanelProps) {
  const squareStyles: Record<string, React.CSSProperties> = {};
  if (lastMove) {
    squareStyles[lastMove.from] = LAST_MOVE_STYLE;
    squareStyles[lastMove.to] = LAST_MOVE_STYLE;
  }
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
    onPieceDrop: ({ sourceSquare, targetSquare }) =>
      onMove && targetSquare ? onMove(sourceSquare, targetSquare) : false,
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
