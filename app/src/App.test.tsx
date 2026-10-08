import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { App } from "./App";
import { createFakeApi, eventsFor } from "./api/fake";
import { foolsMate, operaGame } from "./fixtures";

function setup(options = {}) {
  const api = createFakeApi({ games: [foolsMate, operaGame], ...options });
  render(<App api={api} />);
  return { api, user: userEvent.setup() };
}

async function ready(api: ReturnType<typeof createFakeApi>) {
  await waitFor(() => expect(api.subscribers()).toBe(1));
}

describe("App", () => {
  it("starts on the home screen with the recent games", async () => {
    setup();
    expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /^A vs B/ })).toBeInTheDocument();
  });

  it("reviews a pasted game from the first click to the finished summary", async () => {
    const { api, user } = setup();
    await ready(api);
    await user.type(screen.getByLabelText("PGN text"), "1. f3 e5 2. g4 Qh4#");
    await user.click(screen.getByRole("button", { name: "Review" }));

    // The board and move list appear immediately; nothing is analysed yet.
    expect(await screen.findByRole("status")).toHaveTextContent("Analysing position 1 of 5");
    expect(screen.getByRole("button", { name: "g4" })).toBeInTheDocument();

    // Stream the first part of the review.
    const events = eventsFor(foolsMate, 1);
    act(() => events.slice(0, 5).forEach((e) => api.emit(e))); // positions 0-2, moves 1-2
    expect(screen.getByRole("status")).toHaveTextContent("Analysing position 4 of 5");
    expect(screen.getByRole("button", { name: "f3, Book" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "e5, Book" })).toBeInTheDocument();
    expect(screen.getByLabelText("White accuracy")).toHaveTextContent("–");

    // Finish it: the saved review replaces the streamed one, bringing the accuracy.
    act(() => events.slice(5).forEach((e) => api.emit(e)));
    expect(await screen.findByText("39.9")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "g4, Blunder" })).toBeInTheDocument();
  });

  it("stays on the home screen and explains when a review cannot start", async () => {
    const { user } = setup({ startError: "illegal or unreadable move \"Ke3\" at ply 3" });
    await user.type(screen.getByLabelText("PGN text"), "1. e4 e5 2. Ke3");
    await user.click(screen.getByRole("button", { name: "Review" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("illegal or unreadable move");
    expect(screen.getByRole("heading", { name: "Review a game" })).toBeInTheDocument();
  });

  it("opens a past game and goes back to the list", async () => {
    const { user } = setup();
    await user.click(await screen.findByRole("button", { name: /^Paul Morphy vs Duke/ }));
    expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("Paul Morphy vs Duke");
    expect(screen.getByRole("button", { name: "Rd8#, Best" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "← Back" }));
    expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
  });

  it("starts the next game at its first move", async () => {
    const { user } = setup();
    await user.click(await screen.findByRole("button", { name: /^Paul Morphy vs Duke/ }));
    await user.click(await screen.findByRole("button", { name: "Last position" }));
    await user.click(screen.getByRole("button", { name: "← Back" }));
    await user.click(await screen.findByRole("button", { name: /^A vs B/ }));
    expect(await screen.findByText("The starting position.")).toBeInTheDocument();
  });

  it("cancels a running review on request and shows what was analysed", async () => {
    const { api, user } = setup();
    await ready(api);
    await user.type(screen.getByLabelText("PGN text"), "1. f3");
    await user.click(screen.getByRole("button", { name: "Review" }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(api.calls).toContainEqual(["cancelReview", 1]);
    act(() => api.emit({ kind: "cancelled", job: 1 }));
    expect(await screen.findByText(/Review cancelled/)).toBeInTheDocument();
  });

  it("cancels the review when the user leaves while it is running", async () => {
    const { api, user } = setup();
    await ready(api);
    await user.type(screen.getByLabelText("PGN text"), "1. f3");
    await user.click(screen.getByRole("button", { name: "Review" }));
    await user.click(await screen.findByRole("button", { name: "← Back" }));
    expect(api.calls).toContainEqual(["cancelReview", 1]);
    expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
  });

  it("shows an engine failure during the review", async () => {
    const { api, user } = setup();
    await ready(api);
    await user.type(screen.getByLabelText("PGN text"), "1. f3");
    await user.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByRole("status");
    act(() => api.emit({ kind: "failed", job: 1, message: "Stockfish was not found" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Stockfish was not found");
  });

  it("switches between the games list and the settings", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Games" }));
    expect(await screen.findByRole("heading", { name: "Review a game" })).toBeInTheDocument();
  });
});
