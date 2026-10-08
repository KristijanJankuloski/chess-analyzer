import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createFakeApi, eventsFor } from "../api/fake";
import { foolsMate, operaGame } from "../fixtures";
import type { JobEvent } from "../generated/JobEvent";
import type { ReviewSource } from "../generated/ReviewSource";
import { type JobState, reduceJob, useReviewJob } from "./useReviewJob";

const source: ReviewSource = { kind: "pgn", text: "1. f3 e5 2. g4 Qh4#", game_index: 0 };

function setup(options = {}) {
  const api = createFakeApi({ games: [foolsMate, operaGame], ...options });
  const hook = renderHook(() => useReviewJob(api));
  return { api, ...hook };
}

describe("reduceJob", () => {
  const running = (job: number): JobState => ({
    status: "running",
    job,
    data: {
      game: foolsMate.game,
      evals: [null, null, null, null, null],
      moves: [null, null, null, null],
      accuracy: null,
      opening: null,
      analysed: 0,
      complete: false,
    },
  });

  it("ignores events for other jobs and when nothing is running", () => {
    const event: JobEvent = { kind: "failed", job: 2, message: "x" };
    const state = running(1);
    expect(reduceJob(state, event)).toBe(state);
    const idle: JobState = { status: "idle" };
    expect(reduceJob(idle, { kind: "failed", job: 1, message: "x" })).toBe(idle);
  });

  it("moves to failed and cancelled, keeping what was already analysed", () => {
    const failed = reduceJob(running(1), { kind: "failed", job: 1, message: "engine died" });
    expect(failed).toMatchObject({ status: "failed", message: "engine died" });
    const cancelled = reduceJob(running(1), { kind: "cancelled", job: 1 });
    expect(cancelled.status).toBe("cancelled");
  });
});

describe("useReviewJob", () => {
  it("starts idle", () => {
    const { result } = setup();
    expect(result.current.state).toEqual({ status: "idle" });
  });

  it("shows the game as soon as the review starts and fills in as events stream", async () => {
    const { api, result } = setup();
    await waitFor(() => expect(api.subscribers()).toBe(1));
    await act(() => result.current.start(source));

    expect(result.current.state.status).toBe("running");
    const events = eventsFor(foolsMate, 1);
    act(() => events.slice(0, 3).forEach((e) => api.emit(e))); // a0, a1, m1
    const state = result.current.state;
    if (state.status !== "running") throw new Error("expected running");
    expect(state.data.analysed).toBe(2);
    expect(state.data.moves[0]).toEqual(foolsMate.review.moves[0]);
    expect(state.data.moves[1]).toBeNull();
  });

  it("replaces the streamed data with the saved review when the job completes", async () => {
    const { api, result } = setup();
    await waitFor(() => expect(api.subscribers()).toBe(1));
    await act(() => result.current.start(source));
    act(() => eventsFor(foolsMate, 1).forEach((e) => api.emit(e)));

    await waitFor(() => {
      const state = result.current.state;
      expect(state.status === "complete" && state.data.complete).toBe(true);
    });
    const state = result.current.state;
    if (state.status !== "complete") throw new Error("expected complete");
    expect(state.gameId).toBe(foolsMate.summary.id);
    expect(state.data.accuracy?.white).toBe(foolsMate.review.accuracy.white);
  });

  it("does not lose events that arrive before the start request is answered", async () => {
    const api = createFakeApi({ games: [foolsMate] });
    const slowStart = api.startReview;
    api.startReview = async (s) => {
      const started = await slowStart(s);
      // The backend is quick: two events are out before the response is processed.
      eventsFor(foolsMate, started.job).slice(0, 2).forEach((e) => api.emit(e));
      return started;
    };
    const { result } = renderHook(() => useReviewJob(api));
    await waitFor(() => expect(api.subscribers()).toBe(1));
    await act(() => result.current.start(source));

    const state = result.current.state;
    if (state.status !== "running") throw new Error("expected running");
    expect(state.data.analysed).toBe(2);
    expect(state.data.evals[0]).not.toBeNull();
    expect(state.data.evals[1]).not.toBeNull();
  });

  it("ignores events that belong to another job", async () => {
    const { api, result } = setup();
    await waitFor(() => expect(api.subscribers()).toBe(1));
    await act(() => result.current.start(source));
    act(() => api.emit({ kind: "failed", job: 99, message: "someone else's" }));
    expect(result.current.state.status).toBe("running");
  });

  it("reports a rejected request as a failure with its message", async () => {
    const { result } = setup({ startError: "no game found in the PGN" });
    await act(() => result.current.start(source));
    expect(result.current.state).toEqual({
      status: "failed",
      message: "no game found in the PGN",
      data: null,
    });
  });

  it("surfaces an engine failure during the review", async () => {
    const { api, result } = setup();
    await waitFor(() => expect(api.subscribers()).toBe(1));
    await act(() => result.current.start(source));
    act(() => api.emit({ kind: "failed", job: 1, message: "Stockfish was not found" }));
    expect(result.current.state).toMatchObject({
      status: "failed",
      message: "Stockfish was not found",
    });
  });

  it("asks the backend to cancel the running job, then reflects the cancellation", async () => {
    const { api, result } = setup();
    await waitFor(() => expect(api.subscribers()).toBe(1));
    await act(() => result.current.start(source));
    act(() => result.current.cancel());
    expect(api.calls).toContainEqual(["cancelReview", 1]);
    act(() => api.emit({ kind: "cancelled", job: 1 }));
    expect(result.current.state.status).toBe("cancelled");
  });

  it("does nothing when there is no running job to cancel", () => {
    const { api, result } = setup();
    act(() => result.current.cancel());
    expect(api.calls.some((c) => c[0] === "cancelReview")).toBe(false);
  });

  it("opens a finished game from the history", async () => {
    const { result } = setup();
    await act(() => result.current.open(operaGame.summary.id));
    const state = result.current.state;
    if (state.status !== "complete") throw new Error("expected complete");
    expect(state.data.game.moves).toHaveLength(operaGame.game.moves.length);
  });

  it("reports a game that is no longer saved", async () => {
    const { result } = setup();
    await act(() => result.current.open(12345));
    expect(result.current.state).toMatchObject({ status: "failed", message: "That game is no longer saved." });
  });

  it("can be closed back to idle", async () => {
    const { result } = setup();
    await act(() => result.current.open(foolsMate.summary.id));
    act(() => result.current.close());
    expect(result.current.state).toEqual({ status: "idle" });
  });

  it("unsubscribes when unmounted", async () => {
    const { api, unmount } = setup();
    await waitFor(() => expect(api.subscribers()).toBe(1));
    unmount();
    expect(api.subscribers()).toBe(0);
  });
});
