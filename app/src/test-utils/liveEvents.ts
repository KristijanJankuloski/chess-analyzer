import type { Eval } from "../generated/Eval";
import type { LiveEvent } from "../generated/LiveEvent";
import type { LiveLine } from "../generated/LiveLine";
import type { MoveClass } from "../generated/MoveClass";
import type { MoveReview } from "../generated/MoveReview";

/** Builders for the events and values a live analysis produces, for tests. */

export const cp = (value: number): Eval => ({ kind: "cp", value });

export function line(first: string, eval_: Eval = cp(20), san: string[] = []): LiveLine {
  return { rank: 1, eval: eval_, depth: 20, pv: [first], pv_san: san };
}

export function position(revision: number, index: number, first: string, eval_: Eval = cp(20)): LiveEvent {
  return { kind: "position", revision, index, depth: 20, lines: [line(first, eval_)] };
}

export function review(
  ply: number,
  uci: string,
  cls: MoveClass,
  bestUci: string | null = null,
  accuracy = 90,
): MoveReview {
  return {
    ply,
    move_number: Math.ceil(ply / 2),
    side: ply % 2 === 1 ? "white" : "black",
    san: uci,
    uci,
    class: cls,
    eval_before: cp(0),
    eval_after: cp(0),
    best_uci: bestUci,
    best_san: bestUci,
    best_pv: bestUci ? [bestUci] : [],
    loss: 0,
    accuracy,
    critical: false,
  };
}

export function move(revision: number, r: MoveReview, provisional = false): LiveEvent {
  return { kind: "move", revision, review: r, provisional };
}
