import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createFakeApi } from "../api/fake";
import type { LiveEvent } from "../generated/LiveEvent";
import { useLiveGame } from "./useLiveGame";

const cp = (value: number) => ({ kind: "cp", value }) as const;

function position(revision: number, index: number): LiveEvent {
  return {
    kind: "position",
    revision,
    index,
    depth: 20,
    lines: [{ rank: 1, eval: cp(17), depth: 20, pv: ["e2e4"], pv_san: ["e4"] }],
  };
}

function setup(initial: { moves: string[]; active: boolean }, options = {}) {
  const api = createFakeApi(options);
  const hook = renderHook((props) => useLiveGame(api, props.moves, props.active), {
    initialProps: initial,
  });
  return { api, ...hook };
}

const updates = (api: ReturnType<typeof createFakeApi>) =>
  api.calls.filter((call) => call[0] === "liveUpdate").map((call) => call.slice(1));

describe("useLiveGame", () => {
  it("sends the moves to the backend when the screen is active", async () => {
    const { api } = setup({ moves: ["e2e4"], active: true });
    await waitFor(() => expect(updates(api).length).toBeGreaterThan(0));
    expect(updates(api).at(-1)).toEqual([expect.any(Number), ["e2e4"]]);
    expect(api.liveSubscribers()).toBe(1);
  });

  it("sends nothing, and does not pause, while the screen was never active", async () => {
    const { api } = setup({ moves: ["e2e4"], active: false });
    await waitFor(() => expect(api.liveSubscribers()).toBe(1));
    expect(updates(api)).toEqual([]);
    expect(api.calls.some((call) => call[0] === "livePause")).toBe(false);
  });

  it("sends a newer revision each time the moves change", async () => {
    const { api, rerender } = setup({ moves: ["e2e4"], active: true });
    await waitFor(() => expect(updates(api).length).toBeGreaterThan(0));
    const first = updates(api).at(-1)![0] as number;

    rerender({ moves: ["e2e4", "e7e5"], active: true });
    await waitFor(() => expect(updates(api).at(-1)![1]).toEqual(["e2e4", "e7e5"]));
    expect(updates(api).at(-1)![0] as number).toBeGreaterThan(first);
  });

  it("shows events of the current revision and drops those of an older one", async () => {
    const { api, result, rerender } = setup({ moves: ["e2e4"], active: true });
    await waitFor(() => expect(updates(api).length).toBeGreaterThan(0));
    const first = updates(api).at(-1)![0] as number;
    act(() => api.emitLive(position(first, 1)));
    expect(result.current.state.positions[1]?.depth).toBe(20);

    rerender({ moves: ["e2e4", "e7e5"], active: true });
    await waitFor(() => expect(result.current.state.revision).toBeGreaterThan(first));
    // What was learned about the shared first position is kept...
    expect(result.current.state.positions[1]?.depth).toBe(20);
    // ...a late event of the old revision changes nothing...
    act(() => api.emitLive(position(first, 2)));
    expect(result.current.state.positions[2]).toBeNull();
    // ...and one of the new revision does.
    act(() => api.emitLive(position(result.current.state.revision, 2)));
    expect(result.current.state.positions[2]?.depth).toBe(20);
  });

  it("pauses when the screen is left and resumes with a newer revision when it returns", async () => {
    const { api, rerender } = setup({ moves: ["e2e4"], active: true });
    await waitFor(() => expect(updates(api).length).toBeGreaterThan(0));
    const before = updates(api).length;

    rerender({ moves: ["e2e4"], active: false });
    await waitFor(() => expect(api.calls.some((call) => call[0] === "livePause")).toBe(true));
    expect(updates(api)).toHaveLength(before);

    rerender({ moves: ["e2e4"], active: true });
    await waitFor(() => expect(updates(api)).toHaveLength(before + 1));
    const [lastRevision, previousRevision] = [updates(api).at(-1)![0], updates(api).at(-2)![0]];
    expect(lastRevision as number).toBeGreaterThan(previousRevision as number);
  });

  it("reports why the backend could not start", async () => {
    const { result } = setup(
      { moves: ["e2e4"], active: true },
      { liveError: "Stockfish was not found" },
    );
    await waitFor(() => expect(result.current.state.error).toBe("Stockfish was not found"));
  });

  it("shows an error event from the engine, and clears it on the next change of moves", async () => {
    const { api, result, rerender } = setup({ moves: ["e2e4"], active: true });
    await waitFor(() => expect(updates(api).length).toBeGreaterThan(0));
    const revision = result.current.state.revision;
    act(() => api.emitLive({ kind: "error", revision, message: "the engine stopped" }));
    expect(result.current.state.error).toBe("the engine stopped");

    rerender({ moves: ["e2e4", "e7e5"], active: true });
    await waitFor(() => expect(result.current.state.error).toBeNull());
  });

  it("can ask for the analysis again with the same moves", async () => {
    const { api, result } = setup({ moves: ["e2e4"], active: true });
    await waitFor(() => expect(updates(api).length).toBeGreaterThan(0));
    const before = updates(api).length;
    act(() => result.current.restart());
    await waitFor(() => expect(updates(api)).toHaveLength(before + 1));
    expect(updates(api).at(-1)![1]).toEqual(["e2e4"]);
  });

  it("stops listening when it is unmounted", async () => {
    const { api, unmount } = setup({ moves: [], active: true });
    await waitFor(() => expect(api.liveSubscribers()).toBe(1));
    unmount();
    expect(api.liveSubscribers()).toBe(0);
  });
});
