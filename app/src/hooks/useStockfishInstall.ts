import { useCallback, useEffect, useRef, useState } from "react";
import { type Api, errorMessage } from "../api/types";
import type { InstallProgress } from "../generated/InstallProgress";
import type { Installed } from "../generated/Installed";

export type InstallResult = { installed: Installed } | { error: string };

export interface StockfishInstall {
  downloading: boolean;
  progress: InstallProgress | null;
  /** Counts finished downloads, so a screen can tell that one finished while it was showing. */
  finished: number;
  /** How the latest finished download ended; null before the first. */
  result: InstallResult | null;
  /** Starts the download. Does nothing while one is already running. */
  start: () => void;
}

/**
 * The Stockfish download. It belongs to `App`, not to the Settings screen, so leaving Settings
 * does not forget a download that is still running (and then offer to start a second one).
 */
export function useStockfishInstall(api: Api): StockfishInstall {
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [outcome, setOutcome] = useState<{ finished: number; result: InstallResult | null }>({
    finished: 0,
    result: null,
  });
  const running = useRef(false);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let gone = false;
    api.onInstallProgress(setProgress).then((off) => {
      if (gone) off();
      else unsubscribe = off;
    });
    return () => {
      gone = true;
      unsubscribe?.();
    };
  }, [api]);

  const start = useCallback(() => {
    if (running.current) return;
    running.current = true;
    setDownloading(true);
    setProgress(null);
    api
      .downloadStockfish()
      .then(
        (installed): InstallResult => ({ installed }),
        (e): InstallResult => ({ error: errorMessage(e) }),
      )
      .then((result) => {
        running.current = false;
        setDownloading(false);
        setOutcome((previous) => ({ finished: previous.finished + 1, result }));
      });
  }, [api]);

  return { downloading, progress, finished: outcome.finished, result: outcome.result, start };
}
