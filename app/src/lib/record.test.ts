import { describe, expect, it } from "vitest";
import {
  type RecordState,
  currentFen,
  emptyRecording,
  gameOver,
  isPromotion,
  recordedGame,
  statusText,
  takeBack,
  toReviewSource,
  tryMove,
  trySan,
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
    expect(gameOver(emptyRecording)).toEqual({ over: false, result: "*", reason: null });
  });

  it("recognises checkmate and refuses further moves", () => {
    const mate = play("f2f3", "e7e5", "g2g4", "d8h4");
    expect(gameOver(mate)).toEqual({ over: true, result: "0-1", reason: "checkmate" });
    expect(tryMove(mate, "a2", "a3")).toBeNull();
  });

  it("recognises White checkmating", () => {
    const mate = play("e2e4", "e7e5", "d1h5", "b8c6", "f1c4", "g8f6", "h5f7");
    expect(gameOver(mate)).toEqual({ over: true, result: "1-0", reason: "checkmate" });
  });

  it("does not end the game on a repeated position nobody has claimed", () => {
    // Knights out and back twice: the starting position has now occurred three times.
    const repeated = play(
      "g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8",
    );
    expect(gameOver(repeated)).toEqual({ over: false, result: "*", reason: null });
    expect(tryMove(repeated, "e2", "e4")).not.toBeNull();
  });

  it("recognises a draw by stalemate", () => {
    // A well-known quickest stalemate (Sam Loyd, 10 moves).
    const stalemate = play(
      "e2e3", "a7a5", "d1h5", "a8a6", "h5a5", "h7h5", "h2h4", "a6h6", "a5c7", "f7f6",
      "c7d7", "e8f7", "d7b7", "d8d3", "b7b8", "d3h7", "b8c8", "f7g6", "c8e6",
    );
    expect(gameOver(stalemate)).toEqual({ over: true, result: "1/2-1/2", reason: "stalemate" });
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

describe("entering a move as text", () => {
  const moveOf = (state: RecordState, text: string) => {
    const result = trySan(state, text);
    if (!("state" in result)) throw new Error(`"${text}" was refused: ${result.error}`);
    return result.state.uciMoves.at(-1);
  };

  it("accepts standard notation and records the move in UCI", () => {
    expect(moveOf(emptyRecording, "e4")).toBe("e2e4");
    expect(moveOf(emptyRecording, "Nf3")).toBe("g1f3");
    expect(moveOf(play("e2e4", "e7e5"), "Bc4")).toBe("f1c4");
  });

  it("does not mind a check or annotation mark, spaces or the wrong case", () => {
    expect(moveOf(emptyRecording, "  e4  ")).toBe("e2e4");
    expect(moveOf(emptyRecording, "e4!?")).toBe("e2e4");
    expect(moveOf(emptyRecording, "nf3")).toBe("g1f3");
    expect(moveOf(play("e2e4", "e7e5", "g1f3", "b8c6"), "bb5")).toBe("f1b5");
  });

  it("reads a lowercase b as a pawn when that is legal", () => {
    expect(moveOf(play("e2e4", "d7d5", "f1b5", "c7c6"), "bxc6")).toBe("b5c6");
    expect(moveOf(play("a2a3", "a7a6"), "b4")).toBe("b2b4");
  });

  it("accepts castling written with letters or zeros", () => {
    const ready = play("e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6");
    expect(moveOf(ready, "O-O")).toBe("e1g1");
    expect(moveOf(ready, "0-0")).toBe("e1g1");
    expect(moveOf(ready, "o-o")).toBe("e1g1");
  });

  it("accepts a capture, an en passant capture and a promotion", () => {
    expect(moveOf(play("e2e4", "d7d5"), "exd5")).toBe("e4d5");
    expect(moveOf(play("e2e4", "a7a6", "e4e5", "d7d5"), "exd6")).toBe("e5d6");
    const promoting = play("h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5");
    expect(moveOf(promoting, "gxh8=Q")).toBe("g7h8q");
    expect(moveOf(promoting, "g8=N")).toBe("g7g8n");
  });

  it("refuses an illegal move and says which one", () => {
    expect(trySan(emptyRecording, "Nf6")).toEqual({ error: "Illegal move: Nf6" });
    expect(trySan(emptyRecording, "e5")).toEqual({ error: "Illegal move: e5" });
    expect(trySan(emptyRecording, "nonsense")).toEqual({ error: "Illegal move: nonsense" });
  });

  it("asks for a move when there is none", () => {
    expect(trySan(emptyRecording, "   ")).toEqual({ error: "Type a move, such as Nf3." });
  });

  it("refuses a move once the game has ended on the board", () => {
    const mated = play("f2f3", "e7e5", "g2g4", "d8h4");
    expect(trySan(mated, "e4")).toEqual({ error: "The game is over." });
  });

  it("leaves the original state alone", () => {
    const before = play("e2e4");
    trySan(before, "e5");
    expect(before.uciMoves).toEqual(["e2e4"]);
  });
});

describe("describing the position", () => {
  it("says whose move it is", () => {
    expect(statusText(emptyRecording)).toBe("White to move");
    expect(statusText(play("e2e4"))).toBe("Black to move");
  });

  it("says how a finished game ended", () => {
    expect(statusText(play("f2f3", "e7e5", "g2g4", "d8h4"))).toBe("Checkmate: 0-1");
  });
});
