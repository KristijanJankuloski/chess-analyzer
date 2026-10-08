import type { Eval } from "../generated/Eval";

/** "+0.34", "-1.20", "M3", "-M3", or "1-0 #" for a finished game. Mirrors `Eval::display` in core. */
export function formatEval(e: Eval): string {
  switch (e.kind) {
    case "cp": {
      const pawns = e.value / 100;
      return `${pawns < 0 ? "-" : "+"}${Math.abs(pawns).toFixed(2)}`;
    }
    case "mate":
      return e.value > 0 ? `M${e.value}` : `-M${-e.value}`;
    case "checkmate":
      return e.value === "white" ? "1-0 #" : "0-1 #";
  }
}

/** White's winning chances in percent (0 to 100). Mirrors `Eval::win_percent` in core. */
export function whiteWinPercent(e: Eval): number {
  switch (e.kind) {
    case "cp":
      return 50 + 50 * (2 / (1 + Math.exp(-0.00368208 * e.value)) - 1);
    case "mate":
      return e.value > 0 ? 100 : 0;
    case "checkmate":
      return e.value === "white" ? 100 : 0;
  }
}

export function formatAccuracy(value: number | null): string {
  return value === null ? "–" : value.toFixed(1);
}

/** "8 Oct 2026" from seconds since the Unix epoch. */
export function formatDate(seconds: number): string {
  return new Date(seconds * 1000).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}
