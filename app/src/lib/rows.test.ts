import { describe, expect, it } from "vitest";
import { foolsMate, operaGame } from "../fixtures";
import type { Game } from "../generated/Game";
import { fromStored, startLive } from "./reviewData";
import { moveRows } from "./rows";

describe("moveRows", () => {
  it("pairs White and Black moves under their move number", () => {
    const rows = moveRows(fromStored(foolsMate));
    expect(rows.map((r) => [r.number, r.white?.san, r.black?.san])).toEqual([
      [1, "f3", "e5"],
      [2, "g4", "Qh4#"],
    ]);
    expect(rows[1].black?.ply).toBe(4);
    expect(rows[0].white?.review?.class).toBe("book");
  });

  it("covers a whole game", () => {
    const rows = moveRows(fromStored(operaGame));
    expect(rows).toHaveLength(17);
    expect(rows[16].white?.san).toBe("Rd8#");
    expect(rows[16].black).toBeNull();
  });

  it("starts with an empty White cell when Black moves first", () => {
    const game: Game = {
      headers: {},
      positions: [
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 7",
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 8",
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 8",
      ],
      moves: [
        { san: "e5", uci: "e7e5" },
        { san: "Nf3", uci: "g1f3" },
      ],
    };
    const rows = moveRows(startLive(game));
    expect(rows).toHaveLength(2);
    expect(rows[0]).toMatchObject({ number: 7, white: null, black: { san: "e5", ply: 1 } });
    expect(rows[1]).toMatchObject({ number: 8, white: { san: "Nf3", ply: 2 }, black: null });
  });

  it("leaves unclassified moves without a review while the analysis is running", () => {
    const rows = moveRows(startLive(foolsMate.game));
    expect(rows[0].white?.review).toBeNull();
    expect(rows[0].white?.san).toBe("f3");
  });
});
