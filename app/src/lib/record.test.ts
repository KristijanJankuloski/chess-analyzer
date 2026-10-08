import { describe, expect, it } from "vitest";
import {
  type RecordState,
  currentFen,
  emptyRecording,
  gameOver,
  isPromotion,
  recordedGame,
  takeBack,
  toReviewSource,
  tryMove,
  turn,
} from "./record";

const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/** Plays UCI moves from the start, failing the test if any is rejected. */
function play(...moves: string[]): RecordState {
  let state = emptyRecording;
  for (const move of moves) {
    const next = tryMove(state, move.slice(0, 2), move.slice(2, 4), move[4]);
    if (!next) throw new Error(`move ${move} was rejected`);
    state = next;
  }
  return state;
}

describe("recording moves", () => {
  it("starts from the initial position with White to move", () => {
    expect(currentFen(emptyRecording)).toBe(START);
    expect(turn(emptyRecording)).toBe("white");
    expect(emptyRecording.uciMoves).toEqual([]);
  });

  it("accepts legal moves and alternates the side to move", () => {
    const state = play("e2e4", "e7e5");
    expect(state.uciMoves).toEqual(["e2e4", "e7e5"]);
    expect(turn(state)).toBe("white");
    expect(turn(play("e2e4"))).toBe("black");
  });

  it("rejects illegal moves and leaves the game untouched", () => {
    expect(tryMove(emptyRecording, "e2", "e5")).toBeNull();
    expect(tryMove(emptyRecording, "e7", "e5")).toBeNull(); // Black's pawn on White's turn
    expect(tryMove(emptyRecording, "e4", "e5")).toBeNull(); // nothing on e4
    expect(tryMove(emptyRecording, "a1", "a3")).toBeNull(); // rook through its own pawn
  });

  it("does not mutate the state it was given", () => {
    const before = play("e2e4");
    tryMove(before, "e7", "e5");
    expect(before.uciMoves).toEqual(["e2e4"]);
  });

  it("records castling in the standard UCI form the engine uses", () => {
    const state = play("e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6", "e1g1");
    expect(state.uciMoves.at(-1)).toBe("e1g1");
    expect(recordedGame(state, {}).moves.at(-1)?.san).toBe("O-O");
  });

  it("records en passant", () => {
    const state = play("e2e4", "a7a6", "e4e5", "d7d5", "e5d6");
    expect(recordedGame(state, {}).moves.at(-1)?.san).toBe("exd6");
  });
});

describe("promotion", () => {
  // The h-pawn marches to g7 with g8 empty: 1.h4 g5 2.hxg5 Nf6 3.g6 a6 4.g7 a5, White to move.
  const ready = play("h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5");

  it("is recognised when a pawn reaches the last rank, by a push or a capture", () => {
    expect(isPromotion(ready, "g7", "g8")).toBe(true);
    expect(isPromotion(ready, "g7", "f8")).toBe(true); // capturing the bishop
    expect(isPromotion(ready, "g7", "h8")).toBe(true); // capturing the rook
    expect(isPromotion(ready, "a2", "a3")).toBe(false);
  });

  it("is not a move until a piece is chosen", () => {
    expect(tryMove(ready, "g7", "g8")).toBeNull();
  });

  it("records the chosen piece in UCI and SAN", () => {
    const state = tryMove(ready, "g7", "g8", "n");
    expect(state?.uciMoves.at(-1)).toBe("g7g8n");
    expect(recordedGame(state!, {}).moves.at(-1)?.san).toMatch(/^g8=N/);
    const capture = tryMove(ready, "g7", "h8", "q");
    expect(capture?.uciMoves.at(-1)).toBe("g7h8q");
  });
});

describe("taking moves back", () => {
  it("removes the last move", () => {
    const state = takeBack(play("e2e4", "e7e5"));
    expect(state.uciMoves).toEqual(["e2e4"]);
    expect(turn(state)).toBe("black");
  });

  it("does nothing on an empty game", () => {
    expect(takeBack(emptyRecording)).toBe(emptyRecording);
  });
});

describe("the end of a game", () => {
  it("is ongoing at the start", () => {
    expect(gameOver(emptyRecording)).toEqual({ over: false, result: "*" });
  });

  it("recognises checkmate and refuses further moves", () => {
    const mate = play("f2f3", "e7e5", "g2g4", "d8h4");
    expect(gameOver(mate)).toEqual({ over: true, result: "0-1" });
    expect(tryMove(mate, "a2", "a3")).toBeNull();
  });

  it("recognises White checkmating", () => {
    const mate = play("e2e4", "e7e5", "d1h5", "b8c6", "f1c4", "g8f6", "h5f7");
    expect(gameOver(mate)).toEqual({ over: true, result: "1-0" });
  });

  it("recognises a draw by stalemate", () => {
    // A well-known quickest stalemate (Sam Loyd, 10 moves).
    const stalemate = play(
      "e2e3", "a7a5", "d1h5", "a8a6", "h5a5", "h7h5", "h2h4", "a6h6", "a5c7", "f7f6",
      "c7d7", "e8f7", "d7b7", "d8d3", "b7b8", "d3h7", "b8c8", "f7g6", "c8e6",
    );
    expect(gameOver(stalemate)).toEqual({ over: true, result: "1/2-1/2" });
  });
});

describe("turning a recording into a game and a review request", () => {
  const state = play("f2f3", "e7e5", "g2g4", "d8h4");

  it("describes every position, as the Rust side does", () => {
    const game = recordedGame(state, { white: "Me", black: "You", result: "0-1" });
    expect(game.positions).toHaveLength(5);
    expect(game.positions[0]).toBe(START);
    expect(game.moves.map((m) => m.san)).toEqual(["f3", "e5", "g4", "Qh4#"]);
    expect(game.headers).toMatchObject({ White: "Me", Black: "You", Result: "0-1" });
  });

  it("asks the backend to review the moves from the standard start", () => {
    const source = toReviewSource(state, { white: "Me", black: "You", result: "0-1" });
    expect(source).toMatchObject({
      kind: "moves",
      start_fen: null,
      uci_moves: ["f2f3", "e7e5", "g2g4", "d8h4"],
    });
    if (source.kind !== "moves") throw new Error("expected moves");
    expect(source.headers).toMatchObject({ White: "Me", Black: "You", Result: "0-1", Event: "Recorded game" });
    expect(source.headers.Date).toMatch(/^\d{4}\.\d{2}\.\d{2}$/);
  });

  it("falls back to sensible names and the game's own result", () => {
    const source = toReviewSource(state, {});
    if (source.kind !== "moves") throw new Error("expected moves");
    expect(source.headers).toMatchObject({ White: "White", Black: "Black", Result: "0-1" });
  });

  it("uses the chosen result for a game that is not over", () => {
    const source = toReviewSource(play("e2e4"), { result: "1-0" });
    if (source.kind !== "moves") throw new Error("expected moves");
    expect(source.headers.Result).toBe("1-0");
    const unset = toReviewSource(play("e2e4"), {});
    if (unset.kind !== "moves") throw new Error("expected moves");
    expect(unset.headers.Result).toBe("*");
  });
});
