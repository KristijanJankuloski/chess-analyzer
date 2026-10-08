import { describe, expect, it } from "vitest";
import { foolsMate, operaGame } from "../fixtures";
import type { JobEvent } from "../generated/JobEvent";
import { applyJobEvent, fromStored, startLive } from "./reviewData";

describe("fromStored", () => {
  it("presents a finished review", () => {
    const data = fromStored(foolsMate);
    expect(data.complete).toBe(true);
    expect(data.evals).toHaveLength(5);
    expect(data.moves).toHaveLength(4);
    expect(data.moves.every((m) => m !== null)).toBe(true);
    expect(data.analysed).toBe(5);
    expect(data.opening).toBe("A00 Barnes Opening: Fool's Mate");
    expect(data.accuracy?.black).toBe(100);
  });

  it("has no opening when the game is not in the book", () => {
    const data = fromStored({ ...foolsMate, review: { ...foolsMate.review, opening: null } });
    expect(data.opening).toBeNull();
  });
});

describe("live reviews", () => {
  it("start empty, with a slot for every position and move", () => {
    const data = startLive(foolsMate.game);
    expect(data.evals).toEqual([null, null, null, null, null]);
    expect(data.moves).toEqual([null, null, null, null]);
    expect(data.analysed).toBe(0);
    expect(data.complete).toBe(false);
    expect(data.accuracy).toBeNull();
  });

  it("fill in as analysed and move events arrive, without mutating the previous state", () => {
    const start = startLive(foolsMate.game);
    const withEval = applyJobEvent(start, {
      kind: "analysed",
      job: 1,
      index: 0,
      total: 5,
      eval: { kind: "cp", value: 30 },
    });
    expect(withEval.evals[0]).toEqual({ kind: "cp", value: 30 });
    expect(withEval.analysed).toBe(1);
    expect(start.evals[0]).toBeNull();

    const withMove = applyJobEvent(withEval, {
      kind: "move",
      job: 1,
      mv: foolsMate.review.moves[0],
    });
    expect(withMove.moves[0]).toEqual(foolsMate.review.moves[0]);
    expect(withEval.moves[0]).toBeNull();
  });

  it("ignore events that do not fit the game", () => {
    const start = startLive(foolsMate.game);
    const outOfRange: JobEvent = {
      kind: "analysed",
      job: 1,
      index: 99,
      total: 5,
      eval: { kind: "cp", value: 0 },
    };
    expect(applyJobEvent(start, outOfRange)).toBe(start);
    const badPly: JobEvent = {
      kind: "move",
      job: 1,
      mv: { ...foolsMate.review.moves[0], ply: 40 },
    };
    expect(applyJobEvent(start, badPly)).toBe(start);
    expect(applyJobEvent(start, { kind: "cancelled", job: 1 })).toBe(start);
  });

  it("end up identical to the stored review when every event is replayed", () => {
    for (const stored of [foolsMate, operaGame]) {
      let data = startLive(stored.game);
      stored.review.evals.forEach((eval_, index) => {
        data = applyJobEvent(data, {
          kind: "analysed",
          job: 1,
          index,
          total: stored.review.evals.length,
          eval: eval_,
        });
        if (index >= 1) {
          data = applyJobEvent(data, { kind: "move", job: 1, mv: stored.review.moves[index - 1] });
        }
      });
      const finished = fromStored(stored);
      expect(data.evals).toEqual(finished.evals);
      expect(data.moves).toEqual(finished.moves);
      expect(data.analysed).toBe(finished.analysed);
    }
  });
});
