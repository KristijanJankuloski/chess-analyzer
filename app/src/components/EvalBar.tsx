import type { Eval } from "../generated/Eval";
import { formatEval, whiteWinPercent } from "../lib/format";

export interface EvalBarProps {
  /** The position's evaluation, or null while it is still being analysed. */
  value: Eval | null;
  /** The side drawn at the bottom of the board. */
  orientation: "white" | "black";
}

/** A vertical bar split by who is winning, with the number on the winning side. */
export function EvalBar({ value, orientation }: EvalBarProps) {
  const whiteShare = value ? whiteWinPercent(value) : 50;
  const label = value ? formatEval(value) : "…";
  const whiteIsAhead = whiteShare >= 50;
  // White sits at the bottom when White is at the bottom of the board.
  const whiteAtBottom = orientation === "white";

  return (
    <div
      className="eval-bar"
      role="img"
      aria-label={value ? `Evaluation ${label}` : "No evaluation yet"}
      style={{ flexDirection: whiteAtBottom ? "column" : "column-reverse" }}
    >
      <div className="eval-bar__black" style={{ height: `${100 - whiteShare}%` }} />
      <div className="eval-bar__white" style={{ height: `${whiteShare}%` }} />
      <span
        className={`eval-bar__label eval-bar__label--${whiteIsAhead === whiteAtBottom ? "bottom" : "top"}`}
        style={{ color: whiteIsAhead ? "#262421" : "#f0f0f0" }}
      >
        {label}
      </span>
    </div>
  );
}
