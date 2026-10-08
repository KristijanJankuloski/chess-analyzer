import type { MoveReview } from "../generated/MoveReview";
import type { ReviewData } from "./reviewData";

export interface MoveCell {
  /** 1-based half-move number. */
  ply: number;
  san: string;
  review: MoveReview | null;
  /** In a live game: the class may still change, or rests on a shallow search. */
  provisional?: boolean;
}

export interface MoveRow {
  number: number;
  white: MoveCell | null;
  black: MoveCell | null;
}

/**
 * Groups the game's moves into rows of "1. e4 e5". Handles games that start with Black to move.
 * `provisional` has one flag per move, for a game that is still being played.
 */
export function moveRows(data: ReviewData, provisional?: boolean[]): MoveRow[] {
  const rows: MoveRow[] = [];
  data.game.moves.forEach((move, i) => {
    const fields = data.game.positions[i].split(" ");
    const blackMoves = fields[1] === "b";
    const number = Number.parseInt(fields[5] ?? "1", 10) || 1;
    const cell: MoveCell = { ply: i + 1, san: move.san, review: data.moves[i] };
    if (provisional) cell.provisional = provisional[i] ?? false;
    const last = rows[rows.length - 1];
    if (blackMoves && last && last.number === number && last.black === null && last.white) {
      last.black = cell;
    } else if (blackMoves) {
      rows.push({ number, white: null, black: cell });
    } else {
      rows.push({ number, white: cell, black: null });
    }
  });
  return rows;
}
