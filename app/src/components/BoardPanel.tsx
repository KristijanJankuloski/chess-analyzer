import { Chess, type Square } from "chess.js";
import { useEffect, useMemo, useState } from "react";
import { Chessboard, type Arrow, type ChessboardOptions } from "react-chessboard";
import type { MoveClass } from "../generated/MoveClass";
import type { SquarePair } from "../lib/board";
import { CLASS_INFO } from "../lib/classes";

export interface BoardPanelProps {
  fen: string;
  orientation: "white" | "black";
  lastMove?: SquarePair | null;
  bestArrow?: SquarePair | null;
  /** A second arrow, in red: the move that should have been played instead of the last one. */
  missArrow?: SquarePair | null;
  /** `provisional` draws the badge as unsettled: the analysis behind it may still change. */
  badge?: { square: string; cls: MoveClass; provisional?: boolean } | null;
  /**
   * Makes the board playable, by dragging or by clicking the piece and then its destination.
   * Return true to accept the move, false to refuse it (a dragged piece snaps back).
   */
  onMove?: (from: string, to: string) => boolean;
}

const LAST_MOVE_STYLE = { backgroundColor: "rgba(255, 213, 0, 0.42)" };
const SELECTED_STYLE = { boxShadow: "inset 0 0 0 4px rgba(129, 182, 76, 0.95)" };
// A dot on an empty square the piece can go to, a ring around a square it can capture on.
const MOVE_TARGET_STYLE = {
  backgroundImage: "radial-gradient(circle, rgba(20, 85, 30, 0.5) 0 19%, transparent 21%)",
};
const CAPTURE_TARGET_STYLE = {
  backgroundImage: "radial-gradient(circle, transparent 0 62%, rgba(20, 85, 30, 0.5) 64%)",
};
const BEST_ARROW_COLOR = "rgba(129, 182, 76, 0.92)";
const MISS_ARROW_COLOR = "rgba(202, 52, 49, 0.88)";

/** Where the piece on `square` can legally go, each with whether the move captures. Empty if it has no moves. */
function legalTargets(fen: string, square: string): Map<string, boolean> {
  const targets = new Map<string, boolean>();
  try {
    for (const move of new Chess(fen).moves({ square: square as Square, verbose: true })) {
      targets.set(move.to, move.isCapture() || move.isEnPassant());
    }
  } catch {
    // Not a position chess.js accepts: there is nothing to suggest.
  }
  return targets;
}

export function BoardPanel({
  fen,
  orientation,
  lastMove,
  bestArrow,
  missArrow,
  badge,
  onMove,
}: BoardPanelProps) {
  const [selected, setSelected] = useState<string | null>(null);
  // A new position means the old selection no longer means anything.
  useEffect(() => setSelected(null), [fen]);
  // The piece being dragged; it gets the same move hints as a selected one.
  const [dragging, setDragging] = useState<string | null>(null);
  const hintFrom = onMove ? (dragging ?? selected) : null;
  const targets = useMemo(
    () => (hintFrom ? legalTargets(fen, hintFrom) : new Map<string, boolean>()),
    [fen, hintFrom],
  );

  const squareStyles: Record<string, React.CSSProperties> = {};
  if (lastMove) {
    squareStyles[lastMove.from] = LAST_MOVE_STYLE;
    squareStyles[lastMove.to] = LAST_MOVE_STYLE;
  }
  for (const [square, captures] of targets) {
    squareStyles[square] = {
      ...squareStyles[square],
      ...(captures ? CAPTURE_TARGET_STYLE : MOVE_TARGET_STYLE),
    };
  }
  if (selected) squareStyles[selected] = { ...squareStyles[selected], ...SELECTED_STYLE };

  const arrows: Arrow[] = [];
  if (missArrow) {
    arrows.push({ startSquare: missArrow.from, endSquare: missArrow.to, color: MISS_ARROW_COLOR });
  }
  if (bestArrow) {
    arrows.push({ startSquare: bestArrow.from, endSquare: bestArrow.to, color: BEST_ARROW_COLOR });
  }

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
    onPieceDrag: ({ square }) => setDragging(square),
    onPieceDragCancel: () => setDragging(null),
    onPieceDrop: ({ sourceSquare, targetSquare }) => {
      setDragging(null);
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
    // react-chessboard skips `squareStyles` when it is given a renderer, so apply them here.
    squareRenderer: ({ square, children }) => (
      <div className="square" data-square={square} style={squareStyles[square]}>
        {children}
        {badge && badge.square === square ? (
          <span
            className={`class-badge${badge.provisional ? " class-badge--provisional" : ""}`}
            style={{ backgroundColor: CLASS_INFO[badge.cls].color }}
            title={
              badge.provisional
                ? `${CLASS_INFO[badge.cls].label} (provisional)`
                : CLASS_INFO[badge.cls].label
            }
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
