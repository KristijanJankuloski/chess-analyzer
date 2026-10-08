import type { Accuracy } from "../generated/Accuracy";
import type { Eval } from "../generated/Eval";
import type { Game } from "../generated/Game";
import type { JobEvent } from "../generated/JobEvent";
import type { MoveReview } from "../generated/MoveReview";
import type { StoredGame } from "../generated/StoredGame";

/**
 * Everything the review screen draws, whether the review is finished or still streaming in.
 * Slots that the engine has not reached yet are `null`.
 */
export interface ReviewData {
  game: Game;
  /** One slot per position; index 0 is the starting position. */
  evals: (Eval | null)[];
  /** One slot per half-move; index `ply - 1`. */
  moves: (MoveReview | null)[];
  accuracy: Accuracy | null;
  opening: string | null;
  /** Positions analysed so far. */
  analysed: number;
  complete: boolean;
}

export function fromStored(stored: StoredGame): ReviewData {
  const { game, review } = stored;
  return {
    game,
    evals: review.evals,
    moves: review.moves,
    accuracy: review.accuracy,
    opening: review.opening ? `${review.opening.eco} ${review.opening.name}` : null,
    analysed: review.evals.length,
    complete: true,
  };
}

/** A review that has just started: nothing analysed yet. */
export function startLive(game: Game): ReviewData {
  return {
    game,
    evals: Array.from({ length: game.positions.length }, () => null),
    moves: Array.from({ length: game.moves.length }, () => null),
    accuracy: null,
    opening: null,
    analysed: 0,
    complete: false,
  };
}

/** Folds one streamed job event into the data. Events for other kinds or out of range are ignored. */
export function applyJobEvent(data: ReviewData, event: JobEvent): ReviewData {
  if (event.kind === "analysed") {
    if (event.index < 0 || event.index >= data.evals.length) return data;
    const evals = data.evals.slice();
    evals[event.index] = event.eval;
    return { ...data, evals, analysed: Math.max(data.analysed, event.index + 1) };
  }
  if (event.kind === "move") {
    const slot = event.mv.ply - 1;
    if (slot < 0 || slot >= data.moves.length) return data;
    const moves = data.moves.slice();
    moves[slot] = event.mv;
    return { ...data, moves };
  }
  return data;
}
