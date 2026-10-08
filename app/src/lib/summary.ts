import type { MoveClass } from "../generated/MoveClass";
import type { MoveReview } from "../generated/MoveReview";
import type { Side } from "../generated/Side";

/** How many of `side`'s classified moves fall in each class. */
export function classCounts(
  moves: (MoveReview | null)[],
  side: Side,
): Partial<Record<MoveClass, number>> {
  const counts: Partial<Record<MoveClass, number>> = {};
  for (const move of moves) {
    if (move && move.side === side) {
      counts[move.class] = (counts[move.class] ?? 0) + 1;
    }
  }
  return counts;
}
