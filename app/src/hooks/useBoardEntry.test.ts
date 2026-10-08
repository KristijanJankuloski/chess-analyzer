import { act, renderHook } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { type RecordState, emptyRecording, tryMove } from "../lib/record";
import { useBoardEntry } from "./useBoardEntry";

function setup(initial: RecordState = emptyRecording) {
  return renderHook(() => {
    const [recording, setRecording] = useState(initial);
    return { recording, entry: useBoardEntry(recording, setRecording) };
  });
}

/** Black pawn on g7 about to promote on g8 after a short game. */
function nearPromotion(): RecordState {
  let state = emptyRecording;
  for (const move of ["h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5"]) {
    state = tryMove(state, move.slice(0, 2), move.slice(2, 4))!;
  }
  return state;
}

describe("useBoardEntry", () => {
  it("records a legal move and reports it accepted", () => {
    const { result } = setup();
    let accepted = false;
    act(() => {
      accepted = result.current.entry.handleMove("e2", "e4");
    });
    expect(accepted).toBe(true);
    expect(result.current.recording.uciMoves).toEqual(["e2e4"]);
  });

  it("refuses an illegal move and records nothing", () => {
    const { result } = setup();
    let accepted = true;
    act(() => {
      accepted = result.current.entry.handleMove("e2", "e5");
    });
    expect(accepted).toBe(false);
    expect(result.current.recording.uciMoves).toEqual([]);
  });

  it("holds a promotion back until a piece is chosen", () => {
    const { result } = setup(nearPromotion());
    let accepted = true;
    act(() => {
      accepted = result.current.entry.handleMove("g7", "g8");
    });
    expect(accepted).toBe(false);
    expect(result.current.entry.pending).toEqual({ from: "g7", to: "g8" });
    expect(result.current.recording.uciMoves).toHaveLength(8);

    act(() => result.current.entry.promote("n"));
    expect(result.current.entry.pending).toBeNull();
    expect(result.current.recording.uciMoves.at(-1)).toBe("g7g8n");
  });

  it("drops a pending promotion when it is cancelled", () => {
    const { result } = setup(nearPromotion());
    act(() => {
      result.current.entry.handleMove("g7", "g8");
    });
    act(() => result.current.entry.cancelPromotion());
    expect(result.current.entry.pending).toBeNull();
    expect(result.current.recording.uciMoves).toHaveLength(8);
  });

  it("ignores a choice when nothing is pending", () => {
    const { result } = setup();
    act(() => result.current.entry.promote("q"));
    expect(result.current.recording.uciMoves).toEqual([]);
  });
});
