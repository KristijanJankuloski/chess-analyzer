import { describe, expect, it } from "vitest";
import type { MoveClass } from "../generated/MoveClass";
import { CLASS_INFO, CLASS_ORDER, isError } from "./classes";

const ALL: MoveClass[] = [
  "book",
  "brilliant",
  "great",
  "best",
  "good",
  "inaccuracy",
  "mistake",
  "miss",
  "blunder",
];

describe("class info", () => {
  it("describes every class the engine can produce", () => {
    for (const c of ALL) {
      expect(CLASS_INFO[c].label.length).toBeGreaterThan(0);
      expect(CLASS_INFO[c].symbol.length).toBeGreaterThan(0);
      expect(CLASS_INFO[c].color).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  it("lists every class exactly once in the summary order", () => {
    expect([...CLASS_ORDER].sort()).toEqual([...ALL].sort());
  });

  it("treats only the losing classes as errors", () => {
    expect(ALL.filter(isError).sort()).toEqual(["blunder", "inaccuracy", "miss", "mistake"]);
  });
});
