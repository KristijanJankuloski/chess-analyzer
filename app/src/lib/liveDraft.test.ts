import { describe, expect, it } from "vitest";
import {
  type DraftStorage,
  LIVE_DRAFT_KEY,
  loadLiveDraft,
  saveLiveDraft,
} from "./liveDraft";
import { type RecordDraft, emptyDraft } from "./record";

function memoryStorage(initial: Record<string, string> = {}): DraftStorage & { data: Map<string, string> } {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
    removeItem: (key) => void data.delete(key),
  };
}

const game: RecordDraft = {
  recording: { uciMoves: ["e2e4", "e7e5", "g1f3"] },
  white: "Magnus",
  black: "Hikaru",
  result: "*",
};

describe("remembering the live game", () => {
  it("brings back what was saved", () => {
    const storage = memoryStorage();
    saveLiveDraft(game, storage);
    expect(loadLiveDraft(storage)).toEqual(game);
  });

  it("starts empty when nothing was saved", () => {
    expect(loadLiveDraft(memoryStorage())).toEqual(emptyDraft);
  });

  it("forgets an empty game instead of saving it", () => {
    const storage = memoryStorage();
    saveLiveDraft(game, storage);
    saveLiveDraft(emptyDraft, storage);
    expect(storage.data.has(LIVE_DRAFT_KEY)).toBe(false);
    expect(loadLiveDraft(storage)).toEqual(emptyDraft);
  });

  it("keeps a game that has names but no moves yet", () => {
    const storage = memoryStorage();
    const named = { ...emptyDraft, white: "Magnus" };
    saveLiveDraft(named, storage);
    expect(loadLiveDraft(storage)).toEqual(named);
  });

  it.each([
    ["text that is not JSON", "{oops"],
    ["JSON that is not an object", "42"],
    ["a missing recording", JSON.stringify({ white: "", black: "", result: "*" })],
    ["moves that are not a list", JSON.stringify({ ...game, recording: { uciMoves: "e2e4" } })],
    ["a move that is not text", JSON.stringify({ ...game, recording: { uciMoves: [1] } })],
    ["a result nobody uses", JSON.stringify({ ...game, result: "won" })],
    ["a name that is not text", JSON.stringify({ ...game, white: 3 })],
    ["a move written oddly", JSON.stringify({ ...game, recording: { uciMoves: ["e2e4zzz"] } })],
    ["a move that is not legal", JSON.stringify({ ...game, recording: { uciMoves: ["e2e5"] } })],
    ["moves that stop making sense", JSON.stringify({ ...game, recording: { uciMoves: ["e2e4", "e2e4"] } })],
  ])("starts empty rather than trust %s", (_, text) => {
    expect(loadLiveDraft(memoryStorage({ [LIVE_DRAFT_KEY]: text }))).toEqual(emptyDraft);
  });

  it("copes with storage that is not there or that throws", () => {
    const broken: DraftStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
      removeItem: () => {
        throw new Error("denied");
      },
    };
    expect(loadLiveDraft(broken)).toEqual(emptyDraft);
    expect(() => saveLiveDraft(game, broken)).not.toThrow();
    expect(() => saveLiveDraft(emptyDraft, broken)).not.toThrow();
    expect(loadLiveDraft(null)).toEqual(emptyDraft);
    expect(() => saveLiveDraft(game, null)).not.toThrow();
  });
});
