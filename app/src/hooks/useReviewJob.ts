import { useCallback, useEffect, useRef, useState } from "react";
import { type Api, errorMessage } from "../api/types";
import type { JobEvent } from "../generated/JobEvent";
import type { ReviewSource } from "../generated/ReviewSource";
import { type ReviewData, applyJobEvent, fromStored, startLive } from "../lib/reviewData";

export type JobState =
  | { status: "idle" }
  | { status: "running"; job: number; data: ReviewData }
  | { status: "complete"; gameId: number; data: ReviewData }
  | { status: "failed"; message: string; data: ReviewData | null }
  | { status: "cancelled"; data: ReviewData };

/** How one streamed event changes the state. Pure, so it can be replayed. */
export function reduceJob(state: JobState, event: JobEvent): JobState {
  if (state.status !== "running" || event.job !== state.job) return state;
  switch (event.kind) {
    case "analysed":
    case "move":
      return { ...state, data: applyJobEvent(state.data, event) };
    case "complete":
      return { status: "complete", gameId: event.game_id, data: state.data };
    case "failed":
      return { status: "failed", message: event.message, data: state.data };
    case "cancelled":
      return { status: "cancelled", data: state.data };
  }
}

export interface ReviewJob {
  state: JobState;
  /** Starts reviewing a game. Resolves once the request was accepted or rejected. */
  start(source: ReviewSource): Promise<void>;
  cancel(): void;
  /** Shows a finished review from the history. */
  open(gameId: number): Promise<void>;
  /** Goes back to having nothing open. */
  close(): void;
}

export function useReviewJob(api: Api): ReviewJob {
  const [state, setState] = useState<JobState>({ status: "idle" });
  // Events can arrive before `startReview` has told us the job id; hold them until it does.
  const pending = useRef<JobEvent[] | null>(null);
  const currentJob = useRef<number | null>(null);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    const handle = (event: JobEvent) => {
      if (currentJob.current === null) {
        pending.current?.push(event);
        return;
      }
      setState((previous) => reduceJob(previous, event));
    };
    api.onJobEvent(handle).then((off) => {
      if (disposed) off();
      else unsubscribe = off;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [api]);

  // When a review completes, swap the streamed data for the saved review (it has the
  // accuracy and opening, which only exist once the whole game is done).
  const completedId = state.status === "complete" && !state.data.complete ? state.gameId : null;
  useEffect(() => {
    if (completedId === null) return;
    let stale = false;
    api.getGame(completedId).then((stored) => {
      if (!stale && stored) setState({ status: "complete", gameId: completedId, data: fromStored(stored) });
    });
    return () => {
      stale = true;
    };
  }, [api, completedId]);

  const start = useCallback(
    async (source: ReviewSource) => {
      pending.current = [];
      currentJob.current = null;
      try {
        const started = await api.startReview(source);
        let next: JobState = {
          status: "running",
          job: started.job,
          data: startLive(started.game),
        };
        for (const event of pending.current ?? []) next = reduceJob(next, event);
        currentJob.current = started.job;
        pending.current = null;
        setState(next);
      } catch (error) {
        pending.current = null;
        setState({ status: "failed", message: errorMessage(error), data: null });
      }
    },
    [api],
  );

  const cancel = useCallback(() => {
    if (state.status === "running") void api.cancelReview(state.job);
  }, [api, state]);

  const open = useCallback(
    async (gameId: number) => {
      currentJob.current = null;
      pending.current = null;
      try {
        const stored = await api.getGame(gameId);
        setState(
          stored
            ? { status: "complete", gameId, data: fromStored(stored) }
            : { status: "failed", message: "That game is no longer saved.", data: null },
        );
      } catch (error) {
        setState({ status: "failed", message: errorMessage(error), data: null });
      }
    },
    [api],
  );

  const close = useCallback(() => {
    currentJob.current = null;
    pending.current = null;
    setState({ status: "idle" });
  }, []);

  return { state, start, cancel, open, close };
}
