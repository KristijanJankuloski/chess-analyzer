import { describe, expect, it } from "vitest";
import { formatAccuracy, formatDate, formatEval, whiteWinPercent } from "./format";

describe("formatEval", () => {
  it("shows pawns with a sign", () => {
    expect(formatEval({ kind: "cp", value: 34 })).toBe("+0.34");
    expect(formatEval({ kind: "cp", value: -120 })).toBe("-1.20");
    expect(formatEval({ kind: "cp", value: 0 })).toBe("+0.00");
  });

  it("shows forced mates for either side", () => {
    expect(formatEval({ kind: "mate", value: 3 })).toBe("M3");
    expect(formatEval({ kind: "mate", value: -3 })).toBe("-M3");
  });

  it("shows a finished game by its winner", () => {
    expect(formatEval({ kind: "checkmate", value: "white" })).toBe("1-0 #");
    expect(formatEval({ kind: "checkmate", value: "black" })).toBe("0-1 #");
  });
});

describe("whiteWinPercent", () => {
  it("is 50 when equal and symmetric around it", () => {
    expect(whiteWinPercent({ kind: "cp", value: 0 })).toBeCloseTo(50, 9);
    const up = whiteWinPercent({ kind: "cp", value: 150 });
    const down = whiteWinPercent({ kind: "cp", value: -150 });
    expect(up).toBeGreaterThan(50);
    expect(up + down).toBeCloseTo(100, 9);
  });

  it("matches the Rust value for one pawn (about 59%)", () => {
    expect(whiteWinPercent({ kind: "cp", value: 100 })).toBeCloseTo(59.1, 0);
  });

  it("pins mates to the extremes", () => {
    expect(whiteWinPercent({ kind: "mate", value: 2 })).toBe(100);
    expect(whiteWinPercent({ kind: "mate", value: -2 })).toBe(0);
    expect(whiteWinPercent({ kind: "checkmate", value: "black" })).toBe(0);
  });
});

describe("other formatting", () => {
  it("formats accuracy, with a dash when there is none", () => {
    expect(formatAccuracy(91.456)).toBe("91.5");
    expect(formatAccuracy(null)).toBe("–");
  });

  it("formats an epoch timestamp as a date", () => {
    expect(formatDate(1_700_000_000)).toMatch(/Nov 2023$/);
  });
});
