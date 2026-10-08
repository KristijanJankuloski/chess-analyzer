import type { EngineStatus } from "../generated/EngineStatus";
import type { GameSummary } from "../generated/GameSummary";
import type { JobEvent } from "../generated/JobEvent";
import type { LiveEvent } from "../generated/LiveEvent";
import type { PgnGameInfo } from "../generated/PgnGameInfo";
import type { ReviewSource } from "../generated/ReviewSource";
import type { Settings } from "../generated/Settings";
import type { StartedJob } from "../generated/StartedJob";
import type { StoredGame } from "../generated/StoredGame";

/** Everything the UI asks of the Rust side. Screens only ever see this interface. */
export interface Api {
  /** Describes the games in a PGN so the user can pick one. Rejects with a message if there are none. */
  parsePgnGames(text: string): Promise<PgnGameInfo[]>;
  readPgnFile(path: string): Promise<string>;
  /** Shows the system file picker; resolves to the chosen path, or null if cancelled. */
  pickPgnFile(): Promise<string | null>;
  /** Starts a review. Progress arrives through `onJobEvent`. Rejects with a message for a bad request. */
  startReview(source: ReviewSource): Promise<StartedJob>;
  cancelReview(job: number): Promise<boolean>;
  listGames(): Promise<GameSummary[]>;
  getGame(id: number): Promise<StoredGame | null>;
  deleteGame(id: number): Promise<boolean>;
  getSettings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;
  checkEngine(): Promise<EngineStatus>;
  /** Subscribes to review events; resolves to a function that unsubscribes. */
  onJobEvent(handler: (event: JobEvent) => void): Promise<() => void>;
  /**
   * Tells the live analysis the complete list of moves (UCI) played so far, starting it on first
   * use. The events that follow carry `revision`, which must grow with every call. Rejects with
   * a message when the engine cannot be started.
   */
  liveUpdate(revision: number, moves: string[]): Promise<void>;
  /** Stops searching while keeping what was learned (the user went to another screen). */
  livePause(): Promise<void>;
  /** Subscribes to live analysis events; resolves to a function that unsubscribes. */
  onLiveEvent(handler: (event: LiveEvent) => void): Promise<() => void>;
}

/** Tauri rejects with whatever the Rust command returned as its error; make that readable. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}
