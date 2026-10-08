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
