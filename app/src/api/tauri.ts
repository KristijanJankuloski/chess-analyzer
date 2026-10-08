import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { JobEvent } from "../generated/JobEvent";
import type { LiveEvent } from "../generated/LiveEvent";
import type { Api } from "./types";

/** The event name the Rust shell emits job events on (see `REVIEW_EVENT` in src-tauri). */
export const REVIEW_EVENT = "review-event";
/** The event name for live analysis events (see `LIVE_EVENT` in src-tauri). */
export const LIVE_EVENT = "live-event";

/** The real implementation, backed by the Rust commands. */
export const tauriApi: Api = {
  parsePgnGames: (text) => invoke("parse_pgn_games", { text }),
  readPgnFile: (path) => invoke("read_pgn_file", { path }),
  pickPgnFile: async () => {
    const chosen = await open({
      multiple: false,
      filters: [{ name: "PGN", extensions: ["pgn", "txt"] }],
    });
    return typeof chosen === "string" ? chosen : null;
  },
  startReview: (source) => invoke("start_review", { source }),
  cancelReview: (job) => invoke("cancel_review", { job }),
  listGames: () => invoke("list_games"),
  getGame: (id) => invoke("get_game", { id }),
  deleteGame: (id) => invoke("delete_game", { id }),
  getSettings: () => invoke("get_settings"),
  saveSettings: (settings) => invoke("save_settings", { settings }),
  checkEngine: () => invoke("check_engine_status"),
  onJobEvent: (handler) =>
    listen<JobEvent>(REVIEW_EVENT, (event) => handler(event.payload)),
  liveUpdate: (revision, moves) => invoke("live_update", { revision, moves }),
  livePause: () => invoke("live_pause"),
  onLiveEvent: (handler) =>
    listen<LiveEvent>(LIVE_EVENT, (event) => handler(event.payload)),
};
