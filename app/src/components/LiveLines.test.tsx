import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LiveLine } from "../generated/LiveLine";
import type { LivePosition } from "../lib/live";
import { LiveLines, numberedLine } from "./LiveLines";

const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const BLACK_TO_MOVE = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";

function line(rank: number, value: number, san: string[]): LiveLine {
  return { rank, eval: { kind: "cp", value }, depth: 22, pv: san.map(() => "x"), pv_san: san };
}

describe("numberedLine", () => {
  it("numbers White's moves and each move after a gap", () => {
    expect(numberedLine(START, ["e4", "e5", "Nf3"])).toBe("1. e4 e5 2. Nf3");
  });

  it("starts with an ellipsis when Black is to move", () => {
    expect(numberedLine(BLACK_TO_MOVE, ["e5", "Nf3", "Nc6"])).toBe("1... e5 2. Nf3 Nc6");
  });

  it("copes with a position that has no move number", () => {
    expect(numberedLine("8/8/8/8/8/8/8/K6k w - -", ["Kb1"])).toBe("1. Kb1");
  });

  it("is empty for an empty line", () => {
    expect(numberedLine(START, [])).toBe("");
  });
});

describe("LiveLines", () => {
  const position: LivePosition = {
    depth: 22,
    lines: [line(1, 31, ["e4", "e5", "Nf3"]), line(2, 20, ["d4", "d5"]), line(3, -5, ["c4"])],
  };

  it("lists each line with its evaluation and moves, and the depth reached", () => {
    render(<LiveLines position={position} fen={START} />);
    expect(screen.getByText("depth 22")).toBeInTheDocument();
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0]).toHaveTextContent("+0.31");
    expect(items[0]).toHaveTextContent("1. e4 e5 2. Nf3");
    expect(items[2]).toHaveTextContent("-0.05");
  });

  it("shows only the first moves of a very long line", () => {
    const long = Array.from({ length: 30 }, (_, i) => `m${i}`);
    render(<LiveLines position={{ depth: 30, lines: [line(1, 0, long)] }} fen={START} />);
    expect(screen.getByRole("listitem")).toHaveTextContent("m9");
    expect(screen.getByRole("listitem")).not.toHaveTextContent("m10");
  });

  it("says it is waiting before the engine has answered", () => {
    render(<LiveLines position={null} fen={START} />);
    expect(screen.getByText("Waiting for the engine…")).toBeInTheDocument();
    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  });

  it("shows a finished game's result instead of a depth", () => {
    const finished: LivePosition = {
      depth: 0,
      lines: [{ rank: 1, eval: { kind: "checkmate", value: "black" }, depth: 0, pv: [], pv_san: [] }],
    };
    render(<LiveLines position={finished} fen={START} />);
    expect(screen.getByText("Game over: 0-1 #")).toBeInTheDocument();
    expect(screen.queryByText(/depth/)).not.toBeInTheDocument();
  });
});
