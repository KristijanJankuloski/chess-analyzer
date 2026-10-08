import { describe, expect, it } from "vitest";
import { foolsMate } from "../fixtures";
import { classCounts } from "./summary";

describe("classCounts", () => {
  it("counts each side's moves by class", () => {
    expect(classCounts(foolsMate.review.moves, "white")).toEqual({ book: 1, blunder: 1 });
    expect(classCounts(foolsMate.review.moves, "black")).toEqual({ book: 2 });
  });

  it("skips moves that are not classified yet", () => {
    expect(classCounts([null, foolsMate.review.moves[1], null], "black")).toEqual({ book: 1 });
    expect(classCounts([], "white")).toEqual({});
  });
});
