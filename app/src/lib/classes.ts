import type { MoveClass } from "../generated/MoveClass";

export interface ClassInfo {
  label: string;
  /** Short mark shown next to a move, in the style of chess annotation. */
  symbol: string;
  color: string;
}

export const CLASS_INFO: Record<MoveClass, ClassInfo> = {
  brilliant: { label: "Brilliant", symbol: "!!", color: "#26c2a3" },
  great: { label: "Great", symbol: "!", color: "#5b8bb2" },
  best: { label: "Best", symbol: "★", color: "#81b64c" },
  good: { label: "Good", symbol: "✓", color: "#96af8b" },
  book: { label: "Book", symbol: "≡", color: "#a88865" },
  inaccuracy: { label: "Inaccuracy", symbol: "?!", color: "#f7c631" },
  mistake: { label: "Mistake", symbol: "?", color: "#e58f2a" },
  miss: { label: "Miss", symbol: "×", color: "#dd6b4f" },
  blunder: { label: "Blunder", symbol: "??", color: "#ca3431" },
};

/** Display order for summaries. */
export const CLASS_ORDER: MoveClass[] = [
  "brilliant",
  "great",
  "best",
  "good",
  "book",
  "inaccuracy",
  "mistake",
  "miss",
  "blunder",
];

/** Classes that mean the player gave something away. */
export function isError(c: MoveClass): boolean {
  return c === "inaccuracy" || c === "mistake" || c === "miss" || c === "blunder";
}
