import { foolsMate, operaGame } from "../fixtures";
import type { EngineStatus } from "../generated/EngineStatus";
import type { GameSummary } from "../generated/GameSummary";
import type { JobEvent } from "../generated/JobEvent";
import type { PgnGameInfo } from "../generated/PgnGameInfo";
import type { Settings } from "../generated/Settings";
import type { StartedJob } from "../generated/StartedJob";
import type { StoredGame } from "../generated/StoredGame";
import type { Api } from "./types";

export const DEFAULT_SETTINGS: Settings = {
  engine_path: null,
  threads: 2,
  hash_mb: 256,
  depth: 20,
  multipv: 3,
};

export interface FakeApi extends Api {
  /** Delivers an event to every subscriber, as the Rust side would. */
  emit(event: JobEvent): void;
  /** How many handlers are currently subscribed. */
  subscribers(): number;
  /** Every call, in order, as `[method, ...args]`. */
  calls: unknown[][];
}

export interface FakeOptions {
  games?: StoredGame[];
  settings?: Settings;
  engine?: EngineStatus;
  /** What `startReview` resolves to. */
  started?: StartedJob;
  /** Make `startReview` reject with this message. */
  startError?: string;
  /** Make `parsePgnGames` resolve to this. */
  pgnGames?: PgnGameInfo[];
  /** What `pickPgnFile` resolves to. */
  pickedPath?: string | null;
  /** What `readPgnFile` resolves to. */
  fileText?: string;
}

function summaryOf(stored: StoredGame): GameSummary {
  return stored.summary;
}

/** A scripted, in-memory `Api` for tests and for previewing the UI without the Rust side. */
export function createFakeApi(options: FakeOptions = {}): FakeApi {
  const games = [...(options.games ?? [])];
  let settings = options.settings ?? DEFAULT_SETTINGS;
  const handlers = new Set<(event: JobEvent) => void>();
  const calls: unknown[][] = [];
  const record = (...call: unknown[]) => calls.push(call);

  return {
    calls,
    emit: (event) => handlers.forEach((handler) => handler(event)),
    subscribers: () => handlers.size,

    parsePgnGames: async (text) => {
      record("parsePgnGames", text);
      return (
        options.pgnGames ?? [
          { index: 0, white: "A", black: "B", result: "*", event: "?", date: "?", moves: 4 },
        ]
      );
    },
    readPgnFile: async (path) => {
      record("readPgnFile", path);
      return options.fileText ?? "1. e4 e5 *";
    },
    pickPgnFile: async () => {
      record("pickPgnFile");
      return options.pickedPath ?? null;
    },
    startReview: async (source) => {
      record("startReview", source);
      if (options.startError) throw options.startError;
      return options.started ?? { job: 1, game: foolsMate.game };
    },
    cancelReview: async (job) => {
      record("cancelReview", job);
      return true;
    },
    listGames: async () => {
      record("listGames");
      return games.map(summaryOf);
    },
    getGame: async (id) => {
      record("getGame", id);
      return games.find((g) => g.summary.id === id) ?? null;
    },
    deleteGame: async (id) => {
      record("deleteGame", id);
      const index = games.findIndex((g) => g.summary.id === id);
      if (index >= 0) games.splice(index, 1);
      return index >= 0;
    },
    getSettings: async () => {
      record("getSettings");
      return settings;
    },
    saveSettings: async (next) => {
      record("saveSettings", next);
      settings = next;
      return next;
    },
    checkEngine: async () => {
      record("checkEngine");
      return options.engine ?? { found: true, name: "Stockfish 19", error: null };
    },
    onJobEvent: async (handler) => {
      record("onJobEvent");
      handlers.add(handler);
      return () => {
        handlers.delete(handler);
      };
    },
  };
}

/**
 * Events that replay a stored review as if the engine were running, in the order core sends
 * them: each position, then the move it completes.
 */
export function eventsFor(stored: StoredGame, job: number): JobEvent[] {
  const events: JobEvent[] = [];
  const total = stored.review.evals.length;
  stored.review.evals.forEach((eval_, index) => {
    events.push({ kind: "analysed", job, index, total, eval: eval_ });
    if (index >= 1) events.push({ kind: "move", job, mv: stored.review.moves[index - 1] });
  });
  events.push({ kind: "complete", job, game_id: stored.summary.id });
  return events;
}

/** The two fixture games, as an `Api` that "analyses" whichever PGN it is given. */
export function createDemoApi(delayMs = 350): FakeApi {
  const api = createFakeApi({ games: [operaGame, foolsMate] });
  let nextJob = 10;
  const startReview: Api["startReview"] = async () => {
    const job = nextJob++;
    const events = eventsFor(operaGame, job);
    events.forEach((event, i) => setTimeout(() => api.emit(event), delayMs * (i + 1)));
    return { job, game: operaGame.game };
  };
  return { ...api, startReview };
}
