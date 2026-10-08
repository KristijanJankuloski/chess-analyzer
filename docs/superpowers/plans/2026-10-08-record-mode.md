# chess-analyzer Record-by-Hand Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user enter a game move by move on the board (for example one played over the board), with the board enforcing the rules, take-back, player names and a result, and then review it through the same pipeline as a PGN.

**Architecture:** Recording is a frontend feature: a small pure module (`lib/record.ts`) holds the moves as UCI strings and uses chess.js only to say what is legal, what the position is and whether the game has ended. A `RecordScreen` drives the existing `BoardPanel` (now playable by dragging or by clicking the piece and then its destination) and, on "Review this game", sends `{ kind: "moves", ... }` to the existing `start_review` command. `core` already accepts that request (`ReviewSource::Moves` and `Game::from_uci_moves`), so no Rust changes are needed.

**Tech Stack:** chess.js 1.4.0 (BSD-2-Clause) on top of the stack of the desktop-app plan.

**Spec:** `docs/superpowers/specs/2026-10-08-chess-analyzer-v1-design.md` ("Record mode", build-order step 3). This plan builds on `2026-10-08-desktop-app.md`; its tasks must be done first.

## Global Constraints

- Licences: chess.js is BSD-2-Clause, compatible with the MIT repo. No GPL dependency (see the desktop-app plan).
- chess.js decides legality and nothing else; the review is done by the Rust core from the recorded UCI moves, so the UCI the frontend produces must be exactly what `core` accepts: castling as the king's two-square move (`e1g1`), en passant as the pawn's move (`e5d6`), promotion with a trailing piece letter (`g7g8n`).
- A game is linear (no variations); a recording always starts from the standard initial position.
- A game that ends on the board (checkmate, stalemate, insufficient material, repetition, the fifty-move rule) accepts no more moves and fixes its own result; any other game takes the result the user picks, `*` by default.
- The recording is plain copyable state (the list of moves); everything else is derived by replaying it, so take-back and reset cannot leave stale state behind.
- Everything runs locally; no new Tauri commands or Rust code.

## Review Focus

1. **Promotion**: dropping a pawn on the last rank must ask which piece, never silently pick one or silently refuse; under-promotion must work; backing out must leave the game untouched. Tests: `is not a move until a piece is chosen` and `records the chosen piece in UCI and SAN` (Task 1), `asks which piece to promote to, then records the move` and `can back out of a promotion` (Task 3).
2. **Castling and en passant** must be recorded in the UCI form the Rust core accepts. Tests: `records castling in the standard UCI form the engine uses` and `records en passant` (Task 1), and the real-app check in Task 5, which records both plus a capturing promotion and has the real core and Stockfish review them.
3. **A game that ends on the board** (mate, stalemate): no more moves, result fixed. Tests: `recognises checkmate and refuses further moves`, `recognises a draw by stalemate` (Task 1), `ends the game on checkmate and stops accepting moves` (Task 3).
4. **Nothing to review / nothing to take back**: the buttons must be disabled on an empty game, and taking back past the start must be impossible. Tests: `does nothing on an empty game` (Task 1), `starts with an empty game and nothing to review` (Task 3).
5. **Click-to-move edge cases**: clicking the same square twice, a refused move onto another own piece, a click on an empty square with nothing selected, and a selection that must not survive a position change. Tests: the `click to move` block in `BoardPanel.test.tsx` (Task 2).

## Conventions for every task

- Run commands from `app/` unless a step says otherwise. Snippets are for Git Bash.
- Tests first: write the test, watch it fail for the stated reason, implement, watch it pass.
- Counts are exact for the code in this plan. The suite before this plan has 126 tests in 18 files.

## File Structure

```
app/
  package.json                      + chess.js, + e2e:record
  src/lib/record.ts          (new)  recording state and rules
  src/lib/record.test.ts     (new)
  src/components/BoardPanel.tsx     + click-to-move
  src/screens/RecordScreen.tsx (new)
  src/screens/RecordScreen.test.tsx (new)
  src/App.tsx                       + the Record view and nav button
  src/styles.css                    + record-mode styles
  e2e/record.mjs             (new)  optional end-to-end check of the real app
```

---

### Task 1: Recording logic (`lib/record`)

**Files:**
- Create: `app/src/lib/record.ts`, `app/src/lib/record.test.ts`
- Modify: `app/package.json`, `app/package-lock.json`

**Interfaces:**
- Produces:
  - `interface RecordState { uciMoves: string[] }`, `emptyRecording`, `type RecordResult = "*" | "1-0" | "0-1" | "1/2-1/2"`, `interface RecordHeaders { white?, black?, result? }`
  - `tryMove(state, from, to, promotion?) -> RecordState | null`: null if the move is illegal, the game is over, or it is a promotion without a piece
  - `isPromotion(state, from, to)`, `takeBack(state)` (returns the same object for an empty game), `currentFen(state)`, `turn(state)`
  - `gameOver(state) -> { over, result }` (checkmate gives the winner; every other ending is `1/2-1/2`)
  - `recordedGame(state, headers) -> Game` (the same shape the Rust core produces, so the existing move list can show it) and `toReviewSource(state, headers) -> ReviewSource` (`{ kind: "moves", start_fen: null, uci_moves, headers }` with White, Black, Event "Recorded game", today's Date and the Result; blank names become "White" and "Black")

- [ ] **Step 1: Add chess.js**

Run: `cd app && npm install --save-exact chess.js@1.4.0`
Expected: `added 1 package`. `package.json` now lists `"chess.js": "1.4.0"`.

- [ ] **Step 2: Write the tests**

`app/src/lib/record.test.ts`:

```ts
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
```

- [ ] **Step 3: Run them and see them fail**

Run: `npx vitest run src/lib/record.test.ts`
Expected: `Failed to resolve import "./record"`.

- [ ] **Step 4: Implement**

`app/src/lib/record.ts`:

```ts
import { Chess } from "chess.js";
import type { Game } from "../generated/Game";
import type { ReviewSource } from "../generated/ReviewSource";

/**
 * A game being recorded by hand: just the moves played so far, in UCI. Everything else is
 * worked out by replaying them, so the state is trivially copyable. chess.js only decides what
 * is legal here; the review itself is done by the Rust core from these same moves.
 */
export interface RecordState {
  uciMoves: string[];
}

export type RecordResult = "*" | "1-0" | "0-1" | "1/2-1/2";

export interface RecordHeaders {
  white?: string;
  black?: string;
  /** The result to record when the game did not end on the board (resignation, agreement...). */
  result?: RecordResult;
}

export const emptyRecording: RecordState = { uciMoves: [] };

function playUci(chess: Chess, uci: string) {
  return chess.move({ from: uci.slice(0, 2), to: uci.slice(2, 4), promotion: uci[4] });
}

function replay(state: RecordState): Chess {
  const chess = new Chess();
  for (const uci of state.uciMoves) playUci(chess, uci);
  return chess;
}

export function currentFen(state: RecordState): string {
  return replay(state).fen();
}

export function turn(state: RecordState): "white" | "black" {
  return replay(state).turn() === "w" ? "white" : "black";
}

/** True if moving the piece on `from` to `to` is a pawn promotion (so a piece must be chosen). */
export function isPromotion(state: RecordState, from: string, to: string): boolean {
  const chess = replay(state);
  const piece = chess.get(from as never);
  if (!piece || piece.type !== "p") return false;
  const lastRank = piece.color === "w" ? "8" : "1";
  return (
    to[1] === lastRank &&
    chess.moves({ square: from as never, verbose: true }).some((move) => move.to === to)
  );
}

/**
 * Plays a move. Returns the new state, or null if the move is illegal, the game is over, or it
 * is a promotion and no piece was chosen.
 */
export function tryMove(
  state: RecordState,
  from: string,
  to: string,
  promotion?: string,
): RecordState | null {
  const chess = replay(state);
  if (chess.isGameOver()) return null;
  if (isPromotion(state, from, to) && !promotion) return null;
  try {
    const move = chess.move({ from, to, promotion });
    return { uciMoves: [...state.uciMoves, `${move.from}${move.to}${move.promotion ?? ""}`] };
  } catch {
    return null;
  }
}

export function takeBack(state: RecordState): RecordState {
  return state.uciMoves.length === 0
    ? state
    : { uciMoves: state.uciMoves.slice(0, -1) };
}

/** Whether the game has ended on the board, and the result if so. */
export function gameOver(state: RecordState): { over: boolean; result: RecordResult } {
  const chess = replay(state);
  if (chess.isCheckmate()) {
    // The side to move has been mated.
    return { over: true, result: chess.turn() === "w" ? "0-1" : "1-0" };
  }
  if (chess.isGameOver()) return { over: true, result: "1/2-1/2" };
  return { over: false, result: "*" };
}

function pgnDate(now: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}.${pad(now.getMonth() + 1)}.${pad(now.getDate())}`;
}

function headerMap(state: RecordState, headers: RecordHeaders): Record<string, string> {
  const ended = gameOver(state);
  return {
    Event: "Recorded game",
    Date: pgnDate(new Date()),
    White: headers.white?.trim() || "White",
    Black: headers.black?.trim() || "Black",
    Result: ended.over ? ended.result : (headers.result ?? "*"),
  };
}

/** The recording as the same `Game` shape the Rust core produces, for showing the move list. */
export function recordedGame(state: RecordState, headers: RecordHeaders): Game {
  const chess = new Chess();
  const positions = [chess.fen()];
  const moves = state.uciMoves.map((uci) => {
    const move = playUci(chess, uci);
    positions.push(chess.fen());
    return { san: move.san, uci };
  });
  return { headers: headerMap(state, headers), positions, moves };
}

/** What to send the backend to review this recording. */
export function toReviewSource(state: RecordState, headers: RecordHeaders): ReviewSource {
  return {
    kind: "moves",
    start_fen: null,
    uci_moves: state.uciMoves,
    headers: headerMap(state, headers),
  };
}
```

- [ ] **Step 5: Run them**

Run: `npx vitest run src/lib/record.test.ts && npx tsc --noEmit`
Expected: `19 passed` and no type errors.

Run: `npx vitest run`
Expected: `19 passed` files, `145 passed` tests.

- [ ] **Step 6: Commit**

```bash
cd ..
git add app/package.json app/package-lock.json app/src/lib/record.ts app/src/lib/record.test.ts
git commit -m "Add the recording logic"
```

---

### Task 2: Click-to-move on the board

**Files:**
- Modify: `app/src/components/BoardPanel.tsx`, `app/src/components/BoardPanel.test.tsx`

**Interfaces:**
- Produces: when `onMove` is given, the board can be played by dragging a piece or by clicking a piece and then its destination. The selected square is outlined. A refused click-move onto another piece selects that piece instead; clicking the selected square again, or any square after a refused move onto an empty square, clears the selection; a new position clears it too. Without `onMove` the board is as before (not playable, clicks do nothing).

- [ ] **Step 1: Write the tests**

Replace `app/src/components/BoardPanel.test.tsx` with this (the six existing tests plus a `click to move` block of seven):

```tsx
import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { lastBoardOptions } from "../test-utils/boardStub";
import { BoardPanel } from "./BoardPanel";

const FEN = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

describe("BoardPanel", () => {
  it("shows the position from the side it was asked to", () => {
    render(<BoardPanel fen={FEN} orientation="black" />);
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", FEN);
    expect(lastBoardOptions().boardOrientation).toBe("black");
  });

  it("highlights both squares of the last move", () => {
    render(<BoardPanel fen={FEN} orientation="white" lastMove={{ from: "e2", to: "e4" }} />);
    const styles = lastBoardOptions().squareStyles ?? {};
    expect(Object.keys(styles).sort()).toEqual(["e2", "e4"]);
  });

  it("draws the best-move arrow only when there is one", () => {
    const { rerender } = render(
      <BoardPanel fen={FEN} orientation="white" bestArrow={{ from: "b1", to: "c3" }} />,
    );
    expect(lastBoardOptions().arrows).toEqual([
      expect.objectContaining({ startSquare: "b1", endSquare: "c3" }),
    ]);
    rerender(<BoardPanel fen={FEN} orientation="white" />);
    expect(lastBoardOptions().arrows).toEqual([]);
  });

  it("marks the class of the last move on its square only", () => {
    render(<BoardPanel fen={FEN} orientation="white" badge={{ square: "g4", cls: "blunder" }} />);
    const renderSquare = lastBoardOptions().squareRenderer!;

    const { container: onSquare } = render(renderSquare({ piece: null, square: "g4", children: "♙" }));
    expect(onSquare.querySelector(".class-badge")).toHaveTextContent("??");
    expect(onSquare.querySelector(".class-badge")).toHaveAttribute("title", "Blunder");

    const { container: elsewhere } = render(renderSquare({ piece: null, square: "e4", children: "♙" }));
    expect(elsewhere.querySelector(".class-badge")).toBeNull();
    expect(elsewhere).toHaveTextContent("♙");
  });

  it("is not playable unless given a move handler", () => {
    const { rerender } = render(<BoardPanel fen={FEN} orientation="white" />);
    expect(lastBoardOptions().allowDragging).toBe(false);
    expect(lastBoardOptions().onPieceDrop!({ piece: { isSparePiece: false, position: "e2", pieceType: "wP" }, sourceSquare: "e2", targetSquare: "e4" })).toBe(false);

    const onMove = vi.fn().mockReturnValue(true);
    rerender(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
    expect(lastBoardOptions().allowDragging).toBe(true);
    const dropped = lastBoardOptions().onPieceDrop!({
      piece: { isSparePiece: false, position: "e2", pieceType: "wP" },
      sourceSquare: "e2",
      targetSquare: "e4",
    });
    expect(dropped).toBe(true);
    expect(onMove).toHaveBeenCalledWith("e2", "e4");
  });

  it("rejects a piece dropped off the board", () => {
    const onMove = vi.fn().mockReturnValue(true);
    render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
    const dropped = lastBoardOptions().onPieceDrop!({
      piece: { isSparePiece: false, position: "e2", pieceType: "wP" },
      sourceSquare: "e2",
      targetSquare: null,
    });
    expect(dropped).toBe(false);
    expect(onMove).not.toHaveBeenCalled();
  });

  describe("click to move", () => {
    const pawn = { pieceType: "wP" };
    const click = (square: string, piece: { pieceType: string } | null) =>
      act(() => lastBoardOptions().onSquareClick!({ piece, square }));

    it("moves a piece with two clicks", () => {
      const onMove = vi.fn().mockReturnValue(true);
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      expect(Object.keys(lastBoardOptions().squareStyles ?? {})).toEqual(["e2"]);
      click("e4", null);
      expect(onMove).toHaveBeenCalledWith("e2", "e4");
      expect(lastBoardOptions().squareStyles).toEqual({});
    });

    it("clears the selection when the same square is clicked again", () => {
      const onMove = vi.fn();
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      click("e2", pawn);
      expect(lastBoardOptions().squareStyles).toEqual({});
      expect(onMove).not.toHaveBeenCalled();
    });

    it("selects a different piece when the move is refused and the square holds a piece", () => {
      const onMove = vi.fn().mockReturnValue(false);
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      click("d2", pawn);
      expect(onMove).toHaveBeenCalledWith("e2", "d2");
      expect(Object.keys(lastBoardOptions().squareStyles ?? {})).toEqual(["d2"]);
    });

    it("forgets the selection when a refused move lands on an empty square", () => {
      const onMove = vi.fn().mockReturnValue(false);
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      click("e5", null);
      expect(lastBoardOptions().squareStyles).toEqual({});
    });

    it("ignores clicks on empty squares when nothing is selected", () => {
      const onMove = vi.fn();
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e5", null);
      expect(onMove).not.toHaveBeenCalled();
      expect(lastBoardOptions().squareStyles).toEqual({});
    });

    it("does nothing on a board that is not playable", () => {
      render(<BoardPanel fen={FEN} orientation="white" />);
      click("e2", pawn);
      expect(lastBoardOptions().squareStyles).toEqual({});
    });

    it("drops the selection when the position changes", () => {
      const onMove = vi.fn().mockReturnValue(true);
      const { rerender } = render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      rerender(<BoardPanel fen={FEN.replace(" w ", " b ")} orientation="white" onMove={onMove} />);
      expect(lastBoardOptions().squareStyles).toEqual({});
    });
  });
});
```

- [ ] **Step 2: Run them and see them fail**

Run: `cd app && npx vitest run src/components/BoardPanel.test.tsx`
Expected: `6 passed`, `7 failed` (every `click to move` test, because the board does not set `onSquareClick` yet).

- [ ] **Step 3: Implement**

Replace `app/src/components/BoardPanel.tsx`:

```tsx
import { useEffect, useState } from "react";
import { Chessboard, type Arrow, type ChessboardOptions } from "react-chessboard";
import type { MoveClass } from "../generated/MoveClass";
import type { SquarePair } from "../lib/board";
import { CLASS_INFO } from "../lib/classes";

export interface BoardPanelProps {
  fen: string;
  orientation: "white" | "black";
  lastMove?: SquarePair | null;
  bestArrow?: SquarePair | null;
  badge?: { square: string; cls: MoveClass } | null;
  /**
   * Makes the board playable, by dragging or by clicking the piece and then its destination.
   * Return true to accept the move, false to refuse it (a dragged piece snaps back).
   */
  onMove?: (from: string, to: string) => boolean;
}

const LAST_MOVE_STYLE = { backgroundColor: "rgba(255, 213, 0, 0.42)" };
const SELECTED_STYLE = { boxShadow: "inset 0 0 0 4px rgba(129, 182, 76, 0.95)" };
const BEST_ARROW_COLOR = "rgba(129, 182, 76, 0.92)";

export function BoardPanel({
  fen,
  orientation,
  lastMove,
  bestArrow,
  badge,
  onMove,
}: BoardPanelProps) {
  const [selected, setSelected] = useState<string | null>(null);
  // A new position means the old selection no longer means anything.
  useEffect(() => setSelected(null), [fen]);

  const squareStyles: Record<string, React.CSSProperties> = {};
  if (lastMove) {
    squareStyles[lastMove.from] = LAST_MOVE_STYLE;
    squareStyles[lastMove.to] = LAST_MOVE_STYLE;
  }
  if (selected) squareStyles[selected] = { ...squareStyles[selected], ...SELECTED_STYLE };

  const arrows: Arrow[] = bestArrow
    ? [{ startSquare: bestArrow.from, endSquare: bestArrow.to, color: BEST_ARROW_COLOR }]
    : [];

  const options: ChessboardOptions = {
    id: "review-board",
    position: fen,
    boardOrientation: orientation,
    squareStyles,
    arrows,
    allowDrawingArrows: false,
    allowDragging: Boolean(onMove),
    showAnimations: true,
    animationDurationInMs: 150,
    darkSquareStyle: { backgroundColor: "#b58863" },
    lightSquareStyle: { backgroundColor: "#f0d9b5" },
    onPieceDrop: ({ sourceSquare, targetSquare }) => {
      setSelected(null);
      return onMove && targetSquare ? onMove(sourceSquare, targetSquare) : false;
    },
    onSquareClick: ({ piece, square }) => {
      if (!onMove) return;
      if (selected && selected !== square && onMove(selected, square)) {
        setSelected(null);
        return;
      }
      // Either nothing was selected, the move was refused, or the same square was clicked again.
      setSelected(piece && square !== selected ? square : null);
    },
    squareRenderer: ({ square, children }) => (
      <div className="square" data-square={square}>
        {children}
        {badge && badge.square === square ? (
          <span
            className="class-badge"
            style={{ backgroundColor: CLASS_INFO[badge.cls].color }}
            title={CLASS_INFO[badge.cls].label}
          >
            {CLASS_INFO[badge.cls].symbol}
          </span>
        ) : null}
      </div>
    ),
  };

  return (
    <div className="board-panel">
      <Chessboard options={options} />
    </div>
  );
}
```

- [ ] **Step 4: Run them**

Run: `npx vitest run src/components/BoardPanel.test.tsx && npx tsc --noEmit`
Expected: `13 passed`, no type errors. The whole suite is now `152 passed`.

- [ ] **Step 5: Commit**

```bash
cd ..
git add app/src/components/BoardPanel.tsx app/src/components/BoardPanel.test.tsx
git commit -m "Let the board be played by clicking as well as dragging"
```

---

### Task 3: The record screen

**Files:**
- Create: `app/src/screens/RecordScreen.tsx`, `app/src/screens/RecordScreen.test.tsx`

**Interfaces:**
- Consumes: `lib/record`, `BoardPanel` (with `onMove`), `MoveList`, `startLive`, `moveRows`.
- Produces: `RecordScreen { onReview(source: ReviewSource), onCancel() }`: a playable board, a status line ("White to move", "Checkmate: 0-1", "Draw"), a promotion chooser (Queen, Rook, Bishop, Knight, Cancel promotion) shown when a pawn reaches the last rank, Take back, New game, Flip board, White and Black name fields, a Result select (fixed and disabled once the game has ended on the board), the move list, and a "Review this game" button that is disabled until a move has been played.

- [ ] **Step 1: Write the tests**

`app/src/screens/RecordScreen.test.tsx`:

```tsx
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { lastBoardOptions } from "../test-utils/boardStub";
import { RecordScreen } from "./RecordScreen";

const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

function setup() {
  const onReview = vi.fn();
  const onCancel = vi.fn();
  render(<RecordScreen onReview={onReview} onCancel={onCancel} />);
  return { onReview, onCancel, user: userEvent.setup() };
}

/** Drops a piece on the (stubbed) board, as dragging would. Returns whether it was accepted. */
function drop(from: string, to: string): boolean {
  let accepted = false;
  act(() => {
    accepted = lastBoardOptions().onPieceDrop!({
      piece: { isSparePiece: false, position: from, pieceType: "wP" },
      sourceSquare: from,
      targetSquare: to,
    });
  });
  return accepted;
}

function playAll(...moves: string[]) {
  for (const move of moves) {
    expect(drop(move.slice(0, 2), move.slice(2, 4)), `move ${move}`).toBe(true);
  }
}

describe("RecordScreen", () => {
  it("starts with an empty game and nothing to review", () => {
    setup();
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", START);
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
    expect(screen.getByRole("button", { name: "Review this game" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Take back" })).toBeDisabled();
    expect(lastBoardOptions().allowDragging).toBe(true);
  });

  it("records the moves played on the board and shows them in the list", () => {
    setup();
    playAll("e2e4", "e7e5");
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
    expect(screen.getByRole("button", { name: "e4" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "e5" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Review this game" })).toBeEnabled();
    expect(screen.getByTestId("chessboard")).toHaveAttribute(
      "data-fen",
      expect.stringContaining("4p3"),
    );
  });

  it("refuses an illegal move and records nothing", () => {
    setup();
    expect(drop("e2", "e5")).toBe(false);
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
    expect(screen.queryByRole("button", { name: "e5" })).not.toBeInTheDocument();
  });

  it("takes the last move back", async () => {
    const { user } = setup();
    playAll("e2e4", "e7e5");
    await user.click(screen.getByRole("button", { name: "Take back" }));
    expect(screen.queryByRole("button", { name: "e5" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Black to move");
  });

  it("asks which piece to promote to, then records the move", async () => {
    const { user } = setup();
    playAll("h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5");

    // Dropping on the last rank is held back until a piece is chosen.
    expect(drop("g7", "g8")).toBe(false);
    expect(screen.getByRole("group", { name: "Promote to" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "g8=N" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Knight" }));
    expect(screen.queryByRole("group", { name: "Promote to" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^g8=N/ })).toBeInTheDocument();
  });

  it("can back out of a promotion", async () => {
    const { user } = setup();
    playAll("h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5");
    drop("g7", "g8");
    await user.click(screen.getByRole("button", { name: "Cancel promotion" }));
    expect(screen.queryByRole("group", { name: "Promote to" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
  });

  it("ends the game on checkmate and stops accepting moves", () => {
    setup();
    playAll("f2f3", "e7e5", "g2g4", "d8h4");
    expect(screen.getByRole("status")).toHaveTextContent("Checkmate: 0-1");
    expect(drop("a2", "a3")).toBe(false);
    expect(screen.getByLabelText("Result")).toBeDisabled();
    expect(screen.getByLabelText("Result")).toHaveValue("0-1");
  });

  it("sends the moves, names and result to be reviewed", async () => {
    const { user, onReview } = setup();
    await user.type(screen.getByLabelText("White"), "Me");
    await user.type(screen.getByLabelText("Black"), "My friend");
    playAll("e2e4", "e7e5");
    await user.selectOptions(screen.getByLabelText("Result"), "1-0");
    await user.click(screen.getByRole("button", { name: "Review this game" }));

    expect(onReview).toHaveBeenCalledOnce();
    const source = onReview.mock.calls[0][0];
    expect(source).toMatchObject({ kind: "moves", start_fen: null, uci_moves: ["e2e4", "e7e5"] });
    expect(source.headers).toMatchObject({ White: "Me", Black: "My friend", Result: "1-0" });
  });

  it("starts over", async () => {
    const { user } = setup();
    playAll("e2e4");
    await user.click(screen.getByRole("button", { name: "New game" }));
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", START);
    expect(screen.queryByRole("button", { name: "e4" })).not.toBeInTheDocument();
  });

  it("flips the board", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Flip board" }));
    expect(lastBoardOptions().boardOrientation).toBe("black");
  });

  it("highlights the last move on the board", () => {
    setup();
    playAll("e2e4");
    expect(Object.keys(lastBoardOptions().squareStyles ?? {}).sort()).toEqual(["e2", "e4"]);
  });

  it("can be cancelled", async () => {
    const { user, onCancel } = setup();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
```

- [ ] **Step 2: Run them and see them fail**

Run: `cd app && npx vitest run src/screens/RecordScreen.test.tsx`
Expected: `Failed to resolve import "./RecordScreen"`.

- [ ] **Step 3: Implement**

`app/src/screens/RecordScreen.tsx`:

```tsx
import { useMemo, useState } from "react";
import { BoardPanel } from "../components/BoardPanel";
import { MoveList } from "../components/MoveList";
import type { ReviewSource } from "../generated/ReviewSource";
import { uciSquares } from "../lib/board";
import {
  type RecordResult,
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
} from "../lib/record";
import { startLive } from "../lib/reviewData";
import { moveRows } from "../lib/rows";

export interface RecordScreenProps {
  onReview: (source: ReviewSource) => void;
  onCancel: () => void;
}

const PROMOTIONS = [
  { label: "Queen", piece: "q" },
  { label: "Rook", piece: "r" },
  { label: "Bishop", piece: "b" },
  { label: "Knight", piece: "n" },
];

const RESULTS: { value: RecordResult; label: string }[] = [
  { value: "*", label: "Unfinished" },
  { value: "1-0", label: "1-0 (White won)" },
  { value: "0-1", label: "0-1 (Black won)" },
  { value: "1/2-1/2", label: "½-½ (draw)" },
];

function statusText(state: RecordState): string {
  const ended = gameOver(state);
  if (!ended.over) return `${turn(state) === "white" ? "White" : "Black"} to move`;
  return ended.result === "1/2-1/2" ? "Draw" : `Checkmate: ${ended.result}`;
}

/** Enter a game move by move on the board, then send it off to be reviewed. */
export function RecordScreen({ onReview, onCancel }: RecordScreenProps) {
  const [recording, setRecording] = useState<RecordState>(emptyRecording);
  const [orientation, setOrientation] = useState<"white" | "black">("white");
  const [white, setWhite] = useState("");
  const [black, setBlack] = useState("");
  const [result, setResult] = useState<RecordResult>("*");
  const [pending, setPending] = useState<{ from: string; to: string } | null>(null);

  const headers = { white, black, result };
  const ended = gameOver(recording);
  const game = useMemo(() => recordedGame(recording, { white, black, result }), [recording, white, black, result]);
  const rows = useMemo(() => moveRows(startLive(game)), [game]);
  const lastUci = recording.uciMoves.at(-1);

  const handleMove = (from: string, to: string): boolean => {
    if (isPromotion(recording, from, to)) {
      setPending({ from, to });
      return false; // the piece snaps back until a promotion piece is chosen
    }
    const next = tryMove(recording, from, to);
    if (!next) return false;
    setRecording(next);
    return true;
  };

  const promote = (piece: string) => {
    if (!pending) return;
    const next = tryMove(recording, pending.from, pending.to, piece);
    if (next) setRecording(next);
    setPending(null);
  };

  return (
    <div className="review">
      <header className="review__header">
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
        <h1>Record a game</h1>
      </header>

      <div className="review__body">
        <div className="review__board">
          <BoardPanel
            fen={currentFen(recording)}
            orientation={orientation}
            lastMove={lastUci ? uciSquares(lastUci) : null}
            onMove={ended.over ? undefined : handleMove}
          />
          <p className="review__commentary" role="status">
            {statusText(recording)}
          </p>
          {pending && (
            <div role="group" aria-label="Promote to" className="record__promotion">
              {PROMOTIONS.map(({ label, piece }) => (
                <button key={piece} type="button" onClick={() => promote(piece)}>
                  {label}
                </button>
              ))}
              <button type="button" onClick={() => setPending(null)}>
                Cancel promotion
              </button>
            </div>
          )}
          <div className="nav-controls">
            <button
              type="button"
              onClick={() => setRecording(takeBack(recording))}
              disabled={recording.uciMoves.length === 0}
            >
              Take back
            </button>
            <button
              type="button"
              onClick={() => {
                setRecording(emptyRecording);
                setPending(null);
              }}
            >
              New game
            </button>
            <span className="nav-controls__spacer" />
            <button
              type="button"
              onClick={() => setOrientation((o) => (o === "white" ? "black" : "white"))}
              aria-label="Flip board"
            >
              ⇅
            </button>
          </div>
        </div>

        <div className="review__side">
          <div className="summary record__details">
            <label htmlFor="record-white">White</label>
            <input id="record-white" type="text" value={white} placeholder="White" onChange={(e) => setWhite(e.target.value)} />
            <label htmlFor="record-black">Black</label>
            <input id="record-black" type="text" value={black} placeholder="Black" onChange={(e) => setBlack(e.target.value)} />
            <label htmlFor="record-result">Result</label>
            <select
              id="record-result"
              value={ended.over ? ended.result : result}
              disabled={ended.over}
              onChange={(e) => setResult(e.target.value as RecordResult)}
            >
              {(ended.over ? RESULTS.filter((r) => r.value === ended.result) : RESULTS).map((r) => (
                <option key={r.value} value={r.value}>
                  {r.label}
                </option>
              ))}
            </select>
          </div>

          <MoveList rows={rows} selectedPly={recording.uciMoves.length} onSelect={() => {}} />

          <button
            type="button"
            className="primary"
            disabled={recording.uciMoves.length === 0}
            onClick={() => onReview(toReviewSource(recording, headers))}
          >
            Review this game
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run them**

Run: `npx vitest run src/screens/RecordScreen.test.tsx && npx tsc --noEmit`
Expected: `12 passed`, no type errors. The whole suite is now `164 passed` in 20 files.

- [ ] **Step 5: Commit**

```bash
cd ..
git add app/src/screens/RecordScreen.tsx app/src/screens/RecordScreen.test.tsx
git commit -m "Add the record screen"
```

---

### Task 4: Wire recording into the app

**Files:**
- Modify: `app/src/App.tsx`, `app/src/App.test.tsx`, `app/src/styles.css`

**Interfaces:**
- Produces: a "Record" button in the navigation bar that opens `RecordScreen`; Cancel returns to the games list; "Review this game" starts the review exactly like a pasted PGN and shows the usual review screen. Starting to record while a review is still running cancels it.

- [ ] **Step 1: Write the tests**

Replace `app/src/App.test.tsx` with this (the nine existing tests plus a `recording a game by hand` block of three):

```tsx
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { lastBoardOptions } from "./test-utils/boardStub";
import { createFakeApi, eventsFor } from "./api/fake";
import { foolsMate, operaGame } from "./fixtures";

function setup(options = {}) {
  const api = createFakeApi({ games: [foolsMate, operaGame], ...options });
  render(<App api={api} />);
  return { api, user: userEvent.setup() };
}

async function ready(api: ReturnType<typeof createFakeApi>) {
  await waitFor(() => expect(api.subscribers()).toBe(1));
}

describe("App", () => {
  it("starts on the home screen with the recent games", async () => {
    setup();
    expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /^A vs B/ })).toBeInTheDocument();
  });

  it("reviews a pasted game from the first click to the finished summary", async () => {
    const { api, user } = setup();
    await ready(api);
    await user.type(screen.getByLabelText("PGN text"), "1. f3 e5 2. g4 Qh4#");
    await user.click(screen.getByRole("button", { name: "Review" }));

    // The board and move list appear immediately; nothing is analysed yet.
    expect(await screen.findByRole("status")).toHaveTextContent("Analysing position 1 of 5");
    expect(screen.getByRole("button", { name: "g4" })).toBeInTheDocument();

    // Stream the first part of the review.
    const events = eventsFor(foolsMate, 1);
    act(() => events.slice(0, 5).forEach((e) => api.emit(e))); // positions 0-2, moves 1-2
    expect(screen.getByRole("status")).toHaveTextContent("Analysing position 4 of 5");
    expect(screen.getByRole("button", { name: "f3, Book" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "e5, Book" })).toBeInTheDocument();
    expect(screen.getByLabelText("White accuracy")).toHaveTextContent("–");

    // Finish it: the saved review replaces the streamed one, bringing the accuracy.
    act(() => events.slice(5).forEach((e) => api.emit(e)));
    expect(await screen.findByText("39.9")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "g4, Blunder" })).toBeInTheDocument();
  });

  it("stays on the home screen and explains when a review cannot start", async () => {
    const { user } = setup({ startError: "illegal or unreadable move \"Ke3\" at ply 3" });
    await user.type(screen.getByLabelText("PGN text"), "1. e4 e5 2. Ke3");
    await user.click(screen.getByRole("button", { name: "Review" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("illegal or unreadable move");
    expect(screen.getByRole("heading", { name: "Review a game" })).toBeInTheDocument();
  });

  it("opens a past game and goes back to the list", async () => {
    const { user } = setup();
    await user.click(await screen.findByRole("button", { name: /^Paul Morphy vs Duke/ }));
    expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("Paul Morphy vs Duke");
    expect(screen.getByRole("button", { name: "Rd8#, Best" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "← Back" }));
    expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
  });

  it("starts the next game at its first move", async () => {
    const { user } = setup();
    await user.click(await screen.findByRole("button", { name: /^Paul Morphy vs Duke/ }));
    await user.click(await screen.findByRole("button", { name: "Last position" }));
    await user.click(screen.getByRole("button", { name: "← Back" }));
    await user.click(await screen.findByRole("button", { name: /^A vs B/ }));
    expect(await screen.findByText("The starting position.")).toBeInTheDocument();
  });

  it("cancels a running review on request and shows what was analysed", async () => {
    const { api, user } = setup();
    await ready(api);
    await user.type(screen.getByLabelText("PGN text"), "1. f3");
    await user.click(screen.getByRole("button", { name: "Review" }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(api.calls).toContainEqual(["cancelReview", 1]);
    act(() => api.emit({ kind: "cancelled", job: 1 }));
    expect(await screen.findByText(/Review cancelled/)).toBeInTheDocument();
  });

  it("cancels the review when the user leaves while it is running", async () => {
    const { api, user } = setup();
    await ready(api);
    await user.type(screen.getByLabelText("PGN text"), "1. f3");
    await user.click(screen.getByRole("button", { name: "Review" }));
    await user.click(await screen.findByRole("button", { name: "← Back" }));
    expect(api.calls).toContainEqual(["cancelReview", 1]);
    expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
  });

  it("shows an engine failure during the review", async () => {
    const { api, user } = setup();
    await ready(api);
    await user.type(screen.getByLabelText("PGN text"), "1. f3");
    await user.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByRole("status");
    act(() => api.emit({ kind: "failed", job: 1, message: "Stockfish was not found" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Stockfish was not found");
  });

  it("switches between the games list and the settings", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Games" }));
    expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
  });

  describe("recording a game by hand", () => {
    const drop = (from: string, to: string) =>
      act(() => {
        lastBoardOptions().onPieceDrop!({
          piece: { isSparePiece: false, position: from, pieceType: "wP" },
          sourceSquare: from,
          targetSquare: to,
        });
      });

    it("opens from the navigation bar and can be cancelled", async () => {
      const { user } = setup();
      await user.click(screen.getByRole("button", { name: "Record" }));
      expect(await screen.findByRole("heading", { name: "Record a game" })).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
    });

    it("cancels a review that is still running when you start recording", async () => {
      const { api, user } = setup();
      await ready(api);
      await user.type(screen.getByLabelText("PGN text"), "1. f3");
      await user.click(screen.getByRole("button", { name: "Review" }));
      await screen.findByRole("status");
      await user.click(screen.getByRole("button", { name: "Record" }));
      expect(api.calls).toContainEqual(["cancelReview", 1]);
      expect(await screen.findByRole("heading", { name: "Record a game" })).toBeInTheDocument();
    });

    it("reviews the recorded moves like any other game", async () => {
      const { api, user } = setup();
      await ready(api);
      await user.click(screen.getByRole("button", { name: "Record" }));
      await screen.findByRole("heading", { name: "Record a game" });
      drop("f2", "f3");
      drop("e7", "e5");
      drop("g2", "g4");
      drop("d8", "h4");
      await user.click(screen.getByRole("button", { name: "Review this game" }));

      expect(await screen.findByRole("status")).toHaveTextContent("Analysing position 1 of 5");
      const call = api.calls.find((c) => c[0] === "startReview");
      expect(call?.[1]).toMatchObject({ kind: "moves", uci_moves: ["f2f3", "e7e5", "g2g4", "d8h4"] });
      act(() => eventsFor(foolsMate, 1).forEach((e) => api.emit(e)));
      expect(await screen.findByText("39.9")).toBeInTheDocument();
    });
  });
});
```

- [ ] **Step 2: Run them and see them fail**

Run: `cd app && npx vitest run src/App.test.tsx`
Expected: `9 passed`, `3 failed` (the nav bar has no "Record" button yet).

- [ ] **Step 3: Implement**

Replace `app/src/App.tsx`:

```tsx
import { useCallback, useState } from "react";
import type { Api } from "./api/types";
import { type JobState, useReviewJob } from "./hooks/useReviewJob";
import type { ReviewData } from "./lib/reviewData";
import { HomeScreen } from "./screens/HomeScreen";
import { RecordScreen } from "./screens/RecordScreen";
import { ReviewScreen } from "./screens/ReviewScreen";
import { SettingsScreen } from "./screens/SettingsScreen";

interface Reviewing {
  data: ReviewData;
  status: "running" | "complete" | "failed" | "cancelled";
  message?: string;
}

/** What the review screen should show, or null when there is nothing to review (yet). */
function reviewing(state: JobState): Reviewing | null {
  switch (state.status) {
    case "idle":
      return null;
    case "failed":
      // A request that was rejected before any analysis has nothing to show; stay on the home screen.
      return state.data ? { data: state.data, status: "failed", message: state.message } : null;
    default:
      return { data: state.data, status: state.status };
  }
}

export function App({ api }: { api: Api }) {
  const job = useReviewJob(api);
  const [view, setView] = useState<"home" | "settings" | "record">("home");
  const { state } = job;
  const current = reviewing(state);
  // A new key each time a review is started or opened, so the screen begins at move 0 again.
  const [reviewKey, setReviewKey] = useState(0);
  const { start: startJob, open: openJob } = job;
  const start = useCallback(
    (source: Parameters<typeof startJob>[0]) => {
      setReviewKey((k) => k + 1);
      return startJob(source);
    },
    [startJob],
  );
  const open = useCallback(
    (gameId: number) => {
      setReviewKey((k) => k + 1);
      return openJob(gameId);
    },
    [openJob],
  );

  const goHome = () => {
    if (state.status === "running") job.cancel();
    job.close();
    setView("home");
  };

  let body: React.ReactNode;
  if (view === "settings") {
    body = <SettingsScreen api={api} onDone={() => setView("home")} />;
  } else if (view === "record" && !current) {
    body = (
      <RecordScreen
        onCancel={() => setView("home")}
        onReview={(source) => {
          setView("home");
          void start(source);
        }}
      />
    );
  } else if (current) {
    body = (
      <ReviewScreen
        key={reviewKey}
        data={current.data}
        status={current.status}
        message={current.message}
        onCancel={job.cancel}
        onBack={goHome}
      />
    );
  } else {
    body = (
      <HomeScreen
        api={api}
        onStart={start}
        onOpen={open}
        notice={state.status === "failed" ? state.message : null}
      />
    );
  }

  return (
    <div className="app">
      <nav className="app__nav">
        <strong className="app__title">Chess Analyzer</strong>
        <button type="button" onClick={goHome} aria-current={view === "home" ? "page" : undefined}>
          Games
        </button>
        <button
          type="button"
          onClick={() => {
            if (state.status === "running") job.cancel();
            job.close();
            setView("record");
          }}
          aria-current={view === "record" ? "page" : undefined}
        >
          Record
        </button>
        <button
          type="button"
          onClick={() => setView("settings")}
          aria-current={view === "settings" ? "page" : undefined}
        >
          Settings
        </button>
      </nav>
      <main className="app__main">{body}</main>
    </div>
  );
}
```

Append the record-mode styles to `app/src/styles.css`:

```css
/* Record mode */

.record__details {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.record__details label {
  color: var(--muted);
  margin-top: 4px;
}

.record__details select {
  width: 100%;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 8px;
}

.record__promotion {
  display: flex;
  gap: 6px;
  align-items: center;
}
```

- [ ] **Step 4: Run the whole suite and build**

Run: `npx vitest run && npm run build`
Expected: `20 passed` files, `167 passed` tests, and a successful build.

- [ ] **Step 5: Commit**

```bash
cd ..
git add app/src/App.tsx app/src/App.test.tsx app/src/styles.css
git commit -m "Add the Record view to the app"
```

---

### Task 5: Run it for real, and document it

**Files:**
- Create: `app/e2e/record.mjs`
- Modify: `app/package.json`, `README.md`

**Interfaces:**
- Produces: `npm run e2e:record`, an optional check that drives the real desktop app: it records a 21-half-move game by clicking the board (with a capturing promotion through the piece chooser, en passant and castling), reviews it with the real Rust core and Stockfish, and checks that fool's mate ends the game and fixes the result.

The unit tests prove the screen against a stubbed board and a fake API. Only this run shows that the moves chess.js allowed are the moves the Rust core and Stockfish accept.

- [ ] **Step 1: Add the script**

`app/e2e/record.mjs`:

```js
// End-to-end check of record-by-hand in the real desktop app: enter a game by clicking the
// board (with a capturing promotion, en passant and castling), review it with the real
// Stockfish, then check that a game that ends on the board locks its result.
//
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run tauri dev   (one terminal)
//   npm run e2e:record                                                                      (another)
import { connect, fail, log, sleep } from "./helpers.mjs";

const game = [
  "h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5", "g7f8q", "e8f8", "g1f3",
  "b8c6", "e2e4", "c6b4", "e4e5", "d7d5", "e5d6", "b4c6", "f1c4", "f8g7", "e1g1",
];
const { page, clickText, shot, finish } = await connect();

const play = async (moves) => {
  for (const move of moves) {
    await page.click(`[data-square="${move.slice(0, 2)}"]`);
    await page.click(`[data-square="${move.slice(2, 4)}"]`);
    if (move.length === 5) {
      await page.waitForSelector('[role="group"][aria-label="Promote to"]');
      await clickText("Queen");
    }
    await sleep(120);
  }
};

await clickText("Record");
await page.waitForSelector('[data-square="e2"]');
await page.type("#record-white", "Me");
await page.type("#record-black", "Practice partner");
await play(game);
const moves = await page.evaluate(() =>
  [...document.querySelectorAll(".move")].map((e) => e.getAttribute("aria-label") ?? "").filter(Boolean),
);
log("recorded:", moves.join(" "));
if (moves.length !== game.length) fail(`expected ${game.length} moves on the board, found ${moves.length}`);
if (!moves.includes("O-O") || !moves.some((m) => m.startsWith("gxf8=Q")) || !moves.includes("exd6")) {
  fail("castling, promotion or en passant was not recorded");
}
await shot("record-entered");

// The real Rust core and Stockfish must accept exactly what the board allowed.
await clickText("Review this game");
await page.waitForSelector(".banner", { timeout: 15000 });
await page.waitForFunction(
  () => !document.querySelector(".banner") && document.querySelector('[aria-label="White accuracy"]')?.textContent !== "–",
  { timeout: 240000, polling: 500 },
);
const title = await page.evaluate(() => document.querySelector("h1").textContent);
const accuracy = await page.evaluate(() => [...document.querySelectorAll(".summary__accuracy")].map((e) => e.textContent));
log("reviewed:", title, "| accuracy:", accuracy.join(" / "));
if (!title.includes("Me vs Practice partner")) fail(`unexpected title ${title}`);
await shot("record-reviewed");

// A game that ends on the board (fool's mate) fixes its own result and takes no more moves.
await clickText("Record");
await page.waitForSelector('[data-square="e2"]');
await play(["f2f3", "e7e5", "g2g4", "d8h4"]);
const result = await page.evaluate(() => {
  const select = document.querySelector("#record-result");
  return { value: select.value, disabled: select.disabled };
});
log("fool's mate result:", JSON.stringify(result));
if (result.value !== "0-1" || !result.disabled) fail("the result of a finished game should be fixed at 0-1");

await finish();
```

In `app/package.json`, add this line to `"scripts"` after `"e2e:review"`:

```json
    "e2e:record": "node e2e/record.mjs"
```

- [ ] **Step 2: Start the app with a debugging port**

You need Stockfish in `engines/`. In one terminal:

PowerShell:

```powershell
cd app
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
npm run tauri dev
```

Git Bash:

```bash
cd app
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run tauri dev
```

- [ ] **Step 3: Run the check**

In a second terminal: `cd app && npm run e2e:record`
Expected output (numbers vary slightly):

```
recorded: h4 g5 hxg5 Nf6 g6 a6 g7 a5 gxf8=Q+ Kxf8 Nf3 Nc6 e4 Nb4 e5 d5 exd6 Nc6 Bc4 Kg7 O-O
reviewed: Me vs Practice partner * | accuracy: 76.0 / 65.7
fool's mate result: {"value":"0-1","disabled":true}
no page errors
```

A line starting `FAILED` or a non-zero exit code means the frontend recorded something the backend did not accept (or the reverse).

- [ ] **Step 4: Look at it, then close it**

In the real window: press Record, click `e2` then `e4` (the piece moves), drag `e7` to `e5`, press Take back, and drag a pawn to the last rank to see the promotion chooser. Close the window and stop `npm run tauri dev` with Ctrl+C.

- [ ] **Step 5: Document it**

In `README.md`:

1. In the "Desktop app" section, add this bullet after the **Games** bullet:

```markdown
- **Record:** play a game over the board here, by clicking a piece and then its square or by dragging. The board enforces the rules (castling, en passant, promotion with a piece chooser) and ends the game at checkmate or a draw. Add the players' names, take moves back, and press "Review this game" to analyse it like any PGN.
```

2. In that section's last paragraph, change the words "`npm run e2e:review` checks the real app end to end" to "`npm run e2e:review` and `npm run e2e:record` check the real app end to end".
3. In the Status line, replace "Recording a game by hand and LLM commentary come next" with "LLM commentary comes next".

- [ ] **Step 6: Final verification**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
cd app && npm test && npm run build && cd ..
git status --short
```

Expected: no formatting diff, no clippy output, CLI `6 passed`, core `133 passed`, `golden` `1 passed; 1 ignored`, `stockfish` `10 passed`, frontend `20 passed` files and `167 passed` tests, a successful build, and `git status` listing only the files this task edited.

- [ ] **Step 7: Commit**

```bash
git add app/e2e/record.mjs app/package.json README.md
git commit -m "Add an end-to-end check of recording a game, and document record mode"
```

---

## Self-review against the spec

- **Record mode** ("same board; chess.js validates each dragged move; take-back; 'Review' sends the moves to `start_review`"): Tasks 1 to 4. It also works by clicking, which the spec did not ask for but a game entered by hand needs.
- **Both ways of getting a game in (PGN or by hand) reach the same review pipeline:** `ReviewSource::Moves` was built in the desktop-app plan; Task 5 proves it with the real core.
- **Linear games only, standard start:** `RecordState` is a list of moves from the initial position; `start_fen` is always null.
- **Errors:** an illegal move is refused and the piece snaps back; a game that is over takes no more moves; an empty game cannot be reviewed.
- **Out of scope here:** loading a recording back for editing after it has been reviewed (a reviewed game is saved like any other and can be reopened, but not continued), recording from a custom position, and clocks.
