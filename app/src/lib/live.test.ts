import { describe, expect, it } from "vitest";
import {
  type LiveState,
  applyLiveEvent,
  commonPrefix,
  initialLive,
  liveBoard,
  liveView,
  startRevision,
} from "./live";
import { type RecordState, recordedGame } from "./record";
import { cp, line, move, position, review } from "../test-utils/liveEvents";

const recording = (...uciMoves: string[]): RecordState => ({ uciMoves });
const game = (...uciMoves: string[]) => recordedGame(recording(...uciMoves), {});

describe("keeping the analysis across a change of moves", () => {
  it("starts empty", () => {
    expect(initialLive).toEqual({
      revision: 0,
      moves: [],
      positions: [null],
      reviews: [],
      provisional: [],
      error: null,
    });
  });

  it("finds how much two lists of moves share", () => {
    expect(commonPrefix(["e2e4", "e7e5"], ["e2e4", "c7c5"])).toBe(1);
    expect(commonPrefix(["e2e4"], ["e2e4", "e7e5"])).toBe(1);
    expect(commonPrefix([], ["e2e4"])).toBe(0);
    expect(commonPrefix(["e2e4"], ["e2e4"])).toBe(1);
  });

  it("makes room for a new move and keeps everything known", () => {
    let state = startRevision(initialLive, 1, ["e2e4"]);
    state = applyLiveEvent(state, position(1, 0, "e2e4"));
    state = applyLiveEvent(state, position(1, 1, "e7e5"));
    state = applyLiveEvent(state, move(1, review(1, "e2e4", "best"), true));

    const next = startRevision(state, 2, ["e2e4", "e7e5"]);
    expect(next.revision).toBe(2);
    expect(next.positions).toHaveLength(3);
    expect(next.positions[0]).not.toBeNull();
    expect(next.positions[1]).not.toBeNull();
    expect(next.positions[2]).toBeNull();
    expect(next.reviews).toHaveLength(2);
    expect(next.reviews[0]?.class).toBe("best");
    expect(next.reviews[1]).toBeNull();
  });

  it("forgets what came after the first difference", () => {
    let state = startRevision(initialLive, 1, ["e2e4", "e7e5"]);
    for (const index of [0, 1, 2]) state = applyLiveEvent(state, position(1, index, "a2a3"));
    state = applyLiveEvent(state, move(1, review(1, "e2e4", "best")));
    state = applyLiveEvent(state, move(1, review(2, "e7e5", "good")));

    const next = startRevision(state, 2, ["e2e4", "c7c5"]);
    expect(next.positions[1]).not.toBeNull();
    expect(next.positions[2]).toBeNull();
    expect(next.reviews[0]?.class).toBe("best");
    expect(next.reviews[1]).toBeNull();
  });

  it("shrinks when a move is taken back and clears an old error", () => {
    let state = startRevision(initialLive, 1, ["e2e4", "e7e5"]);
    state = applyLiveEvent(state, { kind: "error", revision: 1, message: "boom" });
    expect(state.error).toBe("boom");

    const next = startRevision(state, 2, ["e2e4"]);
    expect(next.positions).toHaveLength(2);
    expect(next.reviews).toHaveLength(1);
    expect(next.provisional).toHaveLength(1);
    expect(next.error).toBeNull();
  });
});

describe("applying events", () => {
  const started = (): LiveState => startRevision(initialLive, 3, ["e2e4"]);

  it("stores a position's lines and depth", () => {
    const next = applyLiveEvent(started(), position(3, 1, "e7e5", cp(31)));
    expect(next.positions[1]).toEqual({ depth: 20, lines: [line("e7e5", cp(31))] });
  });

  it("stores a move with its provisional flag", () => {
    const next = applyLiveEvent(started(), move(3, review(1, "e2e4", "best"), true));
    expect(next.reviews[0]?.uci).toBe("e2e4");
    expect(next.provisional[0]).toBe(true);
    const settled = applyLiveEvent(next, move(3, review(1, "e2e4", "best"), false));
    expect(settled.provisional[0]).toBe(false);
  });

  it("ignores events from another revision", () => {
    const state = started();
    expect(applyLiveEvent(state, position(2, 1, "e7e5"))).toBe(state);
    expect(applyLiveEvent(state, position(4, 1, "e7e5"))).toBe(state);
    expect(applyLiveEvent(state, { kind: "error", revision: 2, message: "old" })).toBe(state);
  });

  it("ignores positions and moves outside the game", () => {
    const state = started();
    expect(applyLiveEvent(state, position(3, 5, "e7e5"))).toBe(state);
    expect(applyLiveEvent(state, position(3, -1, "e7e5"))).toBe(state);
    expect(applyLiveEvent(state, move(3, review(4, "e2e4", "best")))).toBe(state);
    expect(applyLiveEvent(state, move(3, review(0, "e2e4", "best")))).toBe(state);
  });

  it("keeps the error until the next change of moves", () => {
    const next = applyLiveEvent(started(), { kind: "error", revision: 3, message: "no engine" });
    expect(next.error).toBe("no engine");
  });
});

describe("the view of a live game", () => {
  it("has one slot per position and move, whatever has been analysed", () => {
    const view = liveView(initialLive, game("e2e4", "e7e5"));
    expect(view.data.evals).toEqual([null, null, null]);
    expect(view.data.moves).toEqual([null, null]);
    expect(view.provisional).toEqual([true, true]);
    expect(view.positions).toEqual([null, null, null]);
    expect(view.data.complete).toBe(false);
    expect(view.data.analysed).toBe(0);
  });

  it("shows the best line's evaluation for each analysed position", () => {
    let state = startRevision(initialLive, 1, ["e2e4", "e7e5"]);
    state = applyLiveEvent(state, position(1, 0, "e2e4", cp(30)));
    state = applyLiveEvent(state, position(1, 2, "g1f3", cp(-12)));
    const view = liveView(state, game("e2e4", "e7e5"));
    expect(view.data.evals).toEqual([cp(30), null, cp(-12)]);
    expect(view.data.analysed).toBe(2);
  });

  it("ignores analysis that no longer belongs to the displayed moves", () => {
    let state = startRevision(initialLive, 1, ["e2e4", "e7e5"]);
    state = applyLiveEvent(state, position(1, 2, "g1f3", cp(-12)));
    state = applyLiveEvent(state, move(1, review(2, "e7e5", "good")));
    // The player took back e5 and played c5; the screen has not heard back from the engine yet.
    const view = liveView(state, game("e2e4", "c7c5"));
    expect(view.data.evals[2]).toBeNull();
    expect(view.data.moves[1]).toBeNull();
    expect(view.positions[2]).toBeNull();
  });

  it("averages accuracy per side over the moves classified so far", () => {
    let state = startRevision(initialLive, 1, ["e2e4", "e7e5", "g1f3"]);
    state = applyLiveEvent(state, move(1, review(1, "e2e4", "best", null, 100)));
    state = applyLiveEvent(state, move(1, review(2, "e7e5", "good", null, 80)));
    state = applyLiveEvent(state, move(1, review(3, "g1f3", "good", null, 60)));
    const view = liveView(state, game("e2e4", "e7e5", "g1f3"));
    expect(view.data.accuracy).toEqual({ white: 80, black: 80 });
    const none = liveView(initialLive, game());
    expect(none.data.accuracy).toEqual({ white: null, black: null });
  });
});

describe("what the board shows", () => {
  const moves = ["e2e4", "e7e5"];
  function analysed(): LiveState {
    let state = startRevision(initialLive, 1, moves);
    state = applyLiveEvent(state, position(1, 0, "e2e4"));
    state = applyLiveEvent(state, position(1, 1, "e7e5"));
    state = applyLiveEvent(state, position(1, 2, "g1f3"));
    state = applyLiveEvent(state, move(1, review(1, "e2e4", "best", "e2e4"), false));
    state = applyLiveEvent(state, move(1, review(2, "e7e5", "blunder", "c7c5"), true));
    return state;
  }

  it("shows the position, the last move and its badge", () => {
    const board = liveBoard(liveView(analysed(), game(...moves)), 2, true);
    expect(board.fen).toBe(game(...moves).positions[2]);
    expect(board.lastMove).toEqual({ from: "e7", to: "e5" });
    expect(board.badge).toEqual({ square: "e5", cls: "blunder", provisional: true });
  });

  it("draws the engine's best next move and, after a bad move, what should have been played", () => {
    const board = liveBoard(liveView(analysed(), game(...moves)), 2, true);
    expect(board.nextArrow).toEqual({ from: "g1", to: "f3" });
    expect(board.missArrow).toEqual({ from: "c7", to: "c5" });
  });

  it("draws no red arrow after a move that was fine, even if the engine liked another", () => {
    let state = analysed();
    state = applyLiveEvent(state, move(1, review(2, "e7e5", "good", "c7c5"), true));
    const board = liveBoard(liveView(state, game(...moves)), 2, true);
    expect(board.missArrow).toBeNull();
    expect(board.nextArrow).not.toBeNull();
  });

  it("draws a red arrow for each kind of regrettable move", () => {
    for (const cls of ["inaccuracy", "mistake", "miss", "blunder"] as const) {
      let state = analysed();
      state = applyLiveEvent(state, move(1, review(2, "e7e5", cls, "c7c5")));
      expect(liveBoard(liveView(state, game(...moves)), 2, true).missArrow, cls).not.toBeNull();
    }
  });

  it("draws no red arrow when the played move was the engine's", () => {
    let state = analysed();
    state = applyLiveEvent(state, move(1, review(2, "e7e5", "inaccuracy", "e7e5")));
    expect(liveBoard(liveView(state, game(...moves)), 2, true).missArrow).toBeNull();
  });

  it("hides only the engine's best move on request, and still shows what should have been played", () => {
    const board = liveBoard(liveView(analysed(), game(...moves)), 2, false);
    expect(board.nextArrow).toBeNull();
    expect(board.missArrow).toEqual({ from: "c7", to: "c5" });
    expect(board.badge).not.toBeNull();
  });

  it("shows no arrow at all when the best move is hidden and the last move was fine", () => {
    let state = analysed();
    state = applyLiveEvent(state, move(1, review(2, "e7e5", "good", "c7c5")));
    const board = liveBoard(liveView(state, game(...moves)), 2, false);
    expect(board.nextArrow).toBeNull();
    expect(board.missArrow).toBeNull();
  });

  it("shows the start with no badge, last move or red arrow", () => {
    const board = liveBoard(liveView(analysed(), game(...moves)), 0, true);
    expect(board.lastMove).toBeNull();
    expect(board.badge).toBeNull();
    expect(board.missArrow).toBeNull();
    expect(board.nextArrow).toEqual({ from: "e2", to: "e4" });
  });

  it("has no next-move arrow where nothing has been analysed or the game is over", () => {
    expect(liveBoard(liveView(initialLive, game("e2e4")), 1, true).nextArrow).toBeNull();
    let state = startRevision(initialLive, 1, ["f2f3", "e7e5", "g2g4", "d8h4"]);
    state = applyLiveEvent(state, {
      kind: "position",
      revision: 1,
      index: 4,
      depth: 0,
      lines: [{ rank: 1, eval: { kind: "checkmate", value: "black" }, depth: 0, pv: [], pv_san: [] }],
    });
    const mated = liveBoard(liveView(state, game("f2f3", "e7e5", "g2g4", "d8h4")), 4, true);
    expect(mated.nextArrow).toBeNull();
  });

  it("marks a badge settled once its analyses are deep", () => {
    let state = analysed();
    state = applyLiveEvent(state, move(1, review(2, "e7e5", "blunder", "c7c5"), false));
    const board = liveBoard(liveView(state, game(...moves)), 2, true);
    expect(board.badge?.provisional).toBe(false);
  });
});
