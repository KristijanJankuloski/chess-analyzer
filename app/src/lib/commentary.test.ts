import { describe, expect, it } from "vitest";
import { foolsMate } from "../fixtures";
import type { MoveReview } from "../generated/MoveReview";
import { describeMove } from "./commentary";

const base: MoveReview = foolsMate.review.moves[2]; // 2. g4?? with best Nc3

describe("describeMove", () => {
  it("describes the start, and a move still being analysed", () => {
    expect(describeMove(null, 0)).toBe("The starting position.");
    expect(describeMove(null, 3)).toBe("Analysing this move…");
  });

  it("names the mistake, the better move and the cost", () => {
    const text = describeMove(base, 3);
    expect(text).toMatch(/^g4 is a blunder\./);
    expect(text).toContain("Best was Nc3.");
    expect(text).toMatch(/It cost \d+\.\d% win chance\./);
  });

  it("uses the right article for each kind of error", () => {
    const say = (cls: MoveReview["class"]) => describeMove({ ...base, class: cls }, 3);
    expect(say("inaccuracy")).toMatch(/is an inaccuracy\./);
    expect(say("mistake")).toMatch(/is a mistake\./);
    expect(say("miss")).toMatch(/is a miss\./);
  });

  it("praises good moves without suggesting an alternative to the engine's own choice", () => {
    const best = { ...base, class: "best" as const, best_uci: base.uci, best_san: base.san, loss: 0 };
    expect(describeMove(best, 3)).toBe("g4 is the best move.");
    expect(describeMove({ ...best, class: "brilliant" }, 3)).toBe("g4 is a brilliant move.");
    expect(describeMove({ ...best, class: "great" }, 3)).toBe("g4 is a great move.");
    expect(describeMove({ ...best, class: "book" }, 3)).toBe("g4 is a book move.");
  });

  it("mentions a better move after a merely good one", () => {
    const good = { ...base, class: "good" as const, loss: 3 };
    expect(describeMove(good, 3)).toBe("g4 is a good move. Best was Nc3.");
  });

  it("leaves out the cost when it is negligible", () => {
    const text = describeMove({ ...base, class: "inaccuracy", loss: 0.04 }, 3);
    expect(text).not.toContain("cost");
  });
});
