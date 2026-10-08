import type { Accuracy } from "../generated/Accuracy";
import type { Game } from "../generated/Game";
import type { LiveEvent } from "../generated/LiveEvent";
import type { LiveLine } from "../generated/LiveLine";
import type { MoveClass } from "../generated/MoveClass";
import type { MoveReview } from "../generated/MoveReview";
import type { Side } from "../generated/Side";
import { type SquarePair, boardView, uciSquares } from "./board";
import { isError } from "./classes";
import type { ReviewData } from "./reviewData";

/** What the engine has said about one position, at the depth it has reached. */
export interface LivePosition {
  depth: number;
  lines: LiveLine[];
}

/**
 * Everything the live screen knows about the game being followed, built from the stream of
 * `LiveEvent`s. It belongs to one list of moves (`moves`) and one `revision` of it.
 */
export interface LiveState {
  revision: number;
  /** The moves (UCI) that everything below is about. */
  moves: string[];
  /** One slot per position; index 0 is the start. */
  positions: (LivePosition | null)[];
  /** One slot per move; index `ply - 1`. */
  reviews: (MoveReview | null)[];
  /** Per move: its class may still change, or rests on a shallow search. */
  provisional: boolean[];
  /** Why the analysis stopped, if it did. */
  error: string | null;
}

export const initialLive: LiveState = {
  revision: 0,
  moves: [],
  positions: [null],
  reviews: [],
  provisional: [],
  error: null,
};

/** How many leading moves two lists have in common. */
export function commonPrefix(a: readonly string[], b: readonly string[]): number {
  let n = 0;
  while (n < a.length && n < b.length && a[n] === b[n]) n++;
  return n;
}

function fit<T>(items: readonly T[], length: number, fill: T): T[] {
  return Array.from({ length }, (_, i) => (i < items.length ? items[i] : fill));
}

/**
 * Begins a new revision for a changed list of moves. What was learned about the positions and
 * moves the two lists share is kept, because the backend keeps it too; the rest is forgotten.
 */
export function startRevision(state: LiveState, revision: number, moves: string[]): LiveState {
  const kept = commonPrefix(state.moves, moves);
  return {
    revision,
    moves,
    positions: fit(state.positions.slice(0, kept + 1), moves.length + 1, null),
    reviews: fit(state.reviews.slice(0, kept), moves.length, null),
    provisional: fit(state.provisional.slice(0, kept), moves.length, true),
    error: null,
  };
}

/** Folds one event in. Events of any other revision, and events outside the game, change nothing. */
export function applyLiveEvent(state: LiveState, event: LiveEvent): LiveState {
  if (event.revision !== state.revision) return state;
  switch (event.kind) {
    case "position": {
      if (event.index < 0 || event.index >= state.positions.length) return state;
      const positions = state.positions.slice();
      positions[event.index] = { depth: event.depth, lines: event.lines };
      return { ...state, positions };
    }
    case "move": {
      const slot = event.review.ply - 1;
      if (slot < 0 || slot >= state.reviews.length) return state;
      const reviews = state.reviews.slice();
      reviews[slot] = event.review;
      const provisional = state.provisional.slice();
      provisional[slot] = event.provisional;
      return { ...state, reviews, provisional };
    }
    case "error":
      return { ...state, error: event.message };
  }
}

/** Mean accuracy of the classified moves, per side; null for a side that has none yet. */
function accuracyOf(reviews: (MoveReview | null)[]): Accuracy {
  const mean = (side: Side) => {
    const scores = reviews.flatMap((r) => (r && r.side === side ? [r.accuracy] : []));
    return scores.length ? scores.reduce((sum, s) => sum + s, 0) / scores.length : null;
  };
  return { white: mean("white"), black: mean("black") };
}

/** The live game as the screens that draw reviews expect it, plus what only a live game has. */
export interface LiveView {
  data: ReviewData;
  positions: (LivePosition | null)[];
  provisional: boolean[];
}

/**
 * Lines the state up with the game being shown. The moves can change a moment before the
 * backend has answered, so anything the state knows beyond the moves the two share is left out.
 */
export function liveView(state: LiveState, game: Game): LiveView {
  const kept = commonPrefix(
    state.moves,
    game.moves.map((m) => m.uci),
  );
  const positions = game.positions.map((_, i) => (i <= kept ? (state.positions[i] ?? null) : null));
  const reviews = game.moves.map((_, i) => (i < kept ? (state.reviews[i] ?? null) : null));
  const provisional = game.moves.map((_, i) => (i < kept ? (state.provisional[i] ?? true) : true));
  return {
    data: {
      game,
      evals: positions.map((p) => p?.lines[0]?.eval ?? null),
      moves: reviews,
      accuracy: accuracyOf(reviews),
      opening: null,
      analysed: positions.filter((p) => p !== null).length,
      complete: false,
    },
    positions,
    provisional,
  };
}

export interface LiveBoard {
  fen: string;
  lastMove: SquarePair | null;
  badge: { square: string; cls: MoveClass; provisional: boolean } | null;
  /** The engine's best move for the side to move in this position. */
  nextArrow: SquarePair | null;
  /** The move the last mover should have played, when theirs was an error. */
  missArrow: SquarePair | null;
}

/**
 * What the board shows after `ply` half-moves (0 = the start). `showNext` is about the engine's
 * best move now only; the arrow for the move that should have been played instead of the last
 * one is about the move just made, and is always drawn.
 */
export function liveBoard(view: LiveView, ply: number, showNext: boolean): LiveBoard {
  const base = boardView(view.data, ply, false);
  const best = view.positions[ply]?.lines[0]?.pv[0] ?? null;
  const last = ply > 0 ? view.data.moves[ply - 1] : null;
  const regretted =
    last !== null && isError(last.class) && last.best_uci !== null && last.best_uci !== last.uci;
  return {
    fen: base.fen,
    lastMove: base.lastMove,
    badge: base.badge && { ...base.badge, provisional: view.provisional[ply - 1] ?? true },
    nextArrow: showNext && best ? uciSquares(best) : null,
    missArrow: regretted && last?.best_uci ? uciSquares(last.best_uci) : null,
  };
}
