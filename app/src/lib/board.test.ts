import { describe, expect, it } from "vitest";
import { foolsMate } from "../fixtures";
import { boardView, uciSquares } from "./board";
import { fromStored, startLive } from "./reviewData";

const data = fromStored(foolsMate);

describe("uciSquares", () => {
  it("splits a move into its squares and ignores a promotion piece", () => {
    expect(uciSquares("e2e4")).toEqual({ from: "e2", to: "e4" });
    expect(uciSquares("e7e8q")).toEqual({ from: "e7", to: "e8" });
  });
});

describe("boardView", () => {
  it("shows the start position with nothing highlighted", () => {
    expect(boardView(data, 0, true)).toEqual({
      fen: foolsMate.game.positions[0],
      lastMove: null,
      bestArrow: null,
      badge: null,
    });
  });

  it("marks the last move, its class, and the move the engine preferred", () => {
    const view = boardView(data, 3, true); // 2. g4??
    expect(view.fen).toBe(foolsMate.game.positions[3]);
    expect(view.lastMove).toEqual({ from: "g2", to: "g4" });
    expect(view.badge).toEqual({ square: "g4", cls: "blunder" });
    expect(view.bestArrow).toEqual({ from: "b1", to: "c3" });
  });

  it("hides the arrow when asked to, but keeps the rest", () => {
    const view = boardView(data, 3, false);
    expect(view.bestArrow).toBeNull();
    expect(view.badge?.cls).toBe("blunder");
  });

  it("draws no arrow when the move played was the engine's choice", () => {
    expect(boardView(data, 2, true).bestArrow).toBeNull(); // 1... e5
  });

  it("clamps the ply to the game", () => {
    expect(boardView(data, -4, true).fen).toBe(foolsMate.game.positions[0]);
    expect(boardView(data, 99, true).fen).toBe(foolsMate.game.positions[4]);
  });

  it("has no class badge or arrow for a move that is not classified yet", () => {
    const live = startLive(foolsMate.game);
    const view = boardView(live, 2, true);
    expect(view.lastMove).toEqual({ from: "e7", to: "e5" });
    expect(view.badge).toBeNull();
    expect(view.bestArrow).toBeNull();
  });
});
