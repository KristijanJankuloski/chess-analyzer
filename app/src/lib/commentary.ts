import type { MoveReview } from "../generated/MoveReview";
import { isError } from "./classes";

const ARTICLES = {
  inaccuracy: "an inaccuracy",
  mistake: "a mistake",
  miss: "a miss",
  blunder: "a blunder",
} as const;

/**
 * One plain sentence about a move, from the classification alone. It is what the screens show
 * until the move has commentary of its own (see `commentaryFor`).
 */
export function describeMove(move: MoveReview | null, ply: number): string {
  if (ply === 0) return "The starting position.";
  if (!move) return "Analysing this move…";

  const better =
    move.best_san && move.best_uci !== move.uci ? ` Best was ${move.best_san}.` : "";
  switch (move.class) {
    case "brilliant":
      return `${move.san} is a brilliant move.`;
    case "great":
      return `${move.san} is a great move.`;
    case "best":
      return `${move.san} is the best move.`;
    case "book":
      return `${move.san} is a book move.`;
    case "good":
      return `${move.san} is a good move.${better}`;
    default:
      if (isError(move.class)) {
        const cost = move.loss >= 0.1 ? ` It cost ${move.loss.toFixed(1)}% win chance.` : "";
        return `${move.san} is ${ARTICLES[move.class as keyof typeof ARTICLES]}.${better}${cost}`;
      }
      return move.san;
  }
}

/**
 * What to show under the board for a move: the commentary the backend wrote from the engine's
 * lines when there is some, otherwise the short sentence built from the classification (a review
 * saved before commentary existed, a move still being analysed, or a position that could not be
 * read).
 */
export function commentaryFor(move: MoveReview | null, ply: number): string {
  return move?.commentary ?? describeMove(move, ply);
}
