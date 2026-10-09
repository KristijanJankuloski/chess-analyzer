import { foolsMate, operaGame } from "../fixtures";
import type { EngineStatus } from "../generated/EngineStatus";
import type { GameSummary } from "../generated/GameSummary";
import type { InstallProgress } from "../generated/InstallProgress";
import type { Installed } from "../generated/Installed";
import type { JobEvent } from "../generated/JobEvent";
import type { LiveEvent } from "../generated/LiveEvent";
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
  /** Delivers a live analysis event to every live subscriber. */
  emitLive(event: LiveEvent): void;
  /** Delivers a download progress event to every subscriber. */
  emitInstall(progress: InstallProgress): void;
  /** How many handlers are currently subscribed to review events. */
  subscribers(): number;
  /** How many handlers are currently subscribed to live analysis events. */
  liveSubscribers(): number;
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
  /** Make `liveUpdate` reject with this message (for example, no engine). */
  liveError?: string;
  /** What `downloadStockfish` resolves to. */
  install?: Installed;
  /** Make `downloadStockfish` reject with this message. */
  installError?: string;
}

function summaryOf(stored: StoredGame): GameSummary {
  return stored.summary;
}

/** A scripted, in-memory `Api` for tests and for previewing the UI without the Rust side. */
export function createFakeApi(options: FakeOptions = {}): FakeApi {
  const games = [...(options.games ?? [])];
  let settings = options.settings ?? DEFAULT_SETTINGS;
  const handlers = new Set<(event: JobEvent) => void>();
  const liveHandlers = new Set<(event: LiveEvent) => void>();
  const installHandlers = new Set<(progress: InstallProgress) => void>();
  const calls: unknown[][] = [];
  const record = (...call: unknown[]) => calls.push(call);

  return {
    calls,
    emit: (event) => handlers.forEach((handler) => handler(event)),
    emitLive: (event) => liveHandlers.forEach((handler) => handler(event)),
    emitInstall: (progress) => installHandlers.forEach((handler) => handler(progress)),
    subscribers: () => handlers.size,
    liveSubscribers: () => liveHandlers.size,

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
    downloadStockfish: async () => {
      record("downloadStockfish");
      if (options.installError) throw options.installError;
      const installed = options.install ?? { path: "C:/data/engines/stockfish.exe", engine: "Stockfish 19" };
      settings = { ...settings, engine_path: installed.path };
      return installed;
    },
    onInstallProgress: async (handler) => {
      record("onInstallProgress");
      installHandlers.add(handler);
      return () => {
        installHandlers.delete(handler);
      };
    },
    onJobEvent: async (handler) => {
      record("onJobEvent");
      handlers.add(handler);
      return () => {
        handlers.delete(handler);
      };
    },
    liveUpdate: async (revision, moves) => {
      record("liveUpdate", revision, moves);
      if (options.liveError) throw options.liveError;
    },
    livePause: async () => {
      record("livePause");
    },
    onLiveEvent: async (handler) => {
      record("onLiveEvent");
      liveHandlers.add(handler);
      return () => {
        liveHandlers.delete(handler);
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
