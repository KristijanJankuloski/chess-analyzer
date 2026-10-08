import { type RecordDraft, type RecordResult, emptyDraft, recordedGame } from "./record";

/**
 * The live game is kept in the browser's storage, so closing the app halfway through a
 * tournament game does not lose it. The storage is a convenience: it can be missing, full,
 * blocked or hold anything, so everything here is allowed to fail quietly.
 */
export const LIVE_DRAFT_KEY = "chess-analyzer.live-draft.v1";

export interface DraftStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function browserStorage(): DraftStorage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

const RESULTS: string[] = ["*", "1-0", "0-1", "1/2-1/2"];
const UCI_MOVE = /^[a-h][1-8][a-h][1-8][qrbn]?$/;

/** The saved draft, or null if the text is not a draft whose moves can all be played. */
function parse(text: string): RecordDraft | null {
  try {
    const value: unknown = JSON.parse(text);
    if (typeof value !== "object" || value === null) return null;
    const { recording, white, black, result } = value as Record<string, unknown>;
    const moves = (recording as { uciMoves?: unknown } | null | undefined)?.uciMoves;
    if (!Array.isArray(moves)) return null;
    if (!moves.every((m) => typeof m === "string" && UCI_MOVE.test(m))) return null;
    if (typeof white !== "string" || typeof black !== "string") return null;
    if (typeof result !== "string" || !RESULTS.includes(result)) return null;
    const draft: RecordDraft = {
      recording: { uciMoves: moves as string[] },
      white,
      black,
      result: result as RecordResult,
    };
    recordedGame(draft.recording, {}); // throws if a move is not legal where it stands
    return draft;
  } catch {
    return null;
  }
}

export function loadLiveDraft(storage: DraftStorage | null = browserStorage()): RecordDraft {
  try {
    const text = storage?.getItem(LIVE_DRAFT_KEY);
    return (text ? parse(text) : null) ?? emptyDraft;
  } catch {
    return emptyDraft;
  }
}

export function saveLiveDraft(draft: RecordDraft, storage: DraftStorage | null = browserStorage()): void {
  try {
    if (!storage) return;
    const empty =
      draft.recording.uciMoves.length === 0 && !draft.white && !draft.black && draft.result === "*";
    if (empty) storage.removeItem(LIVE_DRAFT_KEY);
    else storage.setItem(LIVE_DRAFT_KEY, JSON.stringify(draft));
  } catch {
    // Not saved; the game on screen is unaffected.
  }
}
