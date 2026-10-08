import { useCallback, useEffect, useRef, useState } from "react";
import { type Api, errorMessage } from "../api/types";
import { type LiveState, applyLiveEvent, initialLive, startRevision } from "../lib/live";

export interface LiveGame {
  state: LiveState;
  /** Asks for the analysis again with the moves as they are (after the engine stopped). */
  restart(): void;
}

/**
 * Keeps the backend's live analysis in step with `moves` (UCI) and folds its events into state.
 * While `active` is false the search is paused; the state is kept, so coming back to the
 * screen shows what was learned.
 *
 * Every change of moves is sent as a new revision, and the state learns of it before the
 * backend can answer, so an event from an older revision can never be mistaken for a new one.
 */
export function useLiveGame(api: Api, moves: string[], active: boolean): LiveGame {
  const [state, setState] = useState<LiveState>(initialLive);
  const revision = useRef(0);
  const wasActive = useRef(false);
  const latestMoves = useRef(moves);
  latestMoves.current = moves;

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    api
      .onLiveEvent((event) => setState((previous) => applyLiveEvent(previous, event)))
      .then((off) => {
        if (disposed) off();
        else unsubscribe = off;
      });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [api]);

  const send = useCallback(
    (list: string[]) => {
      const next = ++revision.current;
      setState((previous) => startRevision(previous, next, list));
      api.liveUpdate(next, list).catch((error) => {
        setState((previous) =>
          previous.revision === next ? { ...previous, error: errorMessage(error) } : previous,
        );
      });
    },
    [api],
  );

  useEffect(() => {
    if (active) {
      wasActive.current = true;
      send(moves);
    } else if (wasActive.current) {
      wasActive.current = false;
      api.livePause().catch(() => {});
    }
  }, [api, active, moves, send]);

  const restart = useCallback(() => send(latestMoves.current), [send]);

  return { state, restart };
}
