import type { MoveClass } from "../generated/MoveClass";
import type { ReviewData } from "./reviewData";

export interface SquarePair {
  from: string;
  to: string;
}

/** "e2e4" -> e2 to e4; a promotion suffix ("e7e8q") is ignored. */
export function uciSquares(uci: string): SquarePair {
  return { from: uci.slice(0, 2), to: uci.slice(2, 4) };
}

export interface BoardView {
  fen: string;
  /** The move that led to this position. */
  lastMove: SquarePair | null;
  /** What the engine preferred to the move played here. */
  bestArrow: SquarePair | null;
  /** A class mark to draw on the square the last piece landed on. */
  badge: { square: string; cls: MoveClass } | null;
}

/** What the board shows after `ply` half-moves have been played (0 = the start). */
export function boardView(data: ReviewData, ply: number, showBest: boolean): BoardView {
  const clamped = Math.min(Math.max(ply, 0), data.game.positions.length - 1);
  const fen = data.game.positions[clamped];
  if (clamped === 0) return { fen, lastMove: null, bestArrow: null, badge: null };

  const played = data.game.moves[clamped - 1];
  const review = data.moves[clamped - 1];
  const lastMove = uciSquares(played.uci);
  const prefersOther = review?.best_uci != null && review.best_uci !== played.uci;
  return {
    fen,
    lastMove,
    bestArrow: showBest && review && prefersOther ? uciSquares(review.best_uci!) : null,
    badge: review ? { square: lastMove.to, cls: review.class } : null,
  };
}
