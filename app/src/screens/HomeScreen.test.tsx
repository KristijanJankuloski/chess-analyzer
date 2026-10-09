import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { createFakeApi } from "../api/fake";
import { foolsMate, operaGame } from "../fixtures";
import { HomeScreen } from "./HomeScreen";

function setup(options = {}) {
  const api = createFakeApi({ games: [operaGame, foolsMate], ...options });
  const onStart = vi.fn();
  const onOpen = vi.fn();
  const onOpenSettings = vi.fn();
  const view = render(
    <HomeScreen api={api} onStart={onStart} onOpen={onOpen} onOpenSettings={onOpenSettings} />,
  );
  return { api, onStart, onOpen, onOpenSettings, user: userEvent.setup(), ...view };
}

describe("HomeScreen", () => {
  it("cannot review until there is some text", async () => {
    const { user } = setup();
    const review = screen.getByRole("button", { name: "Review" });
    expect(review).toBeDisabled();
    await user.type(screen.getByLabelText("PGN text"), "1. e4");
    expect(review).toBeEnabled();
  });

  it("starts reviewing a single game straight away", async () => {
    const { user, onStart } = setup();
    await user.type(screen.getByLabelText("PGN text"), "1. e4 e5");
    await user.click(screen.getByRole("button", { name: "Review" }));
    await waitFor(() =>
      expect(onStart).toHaveBeenCalledWith({ kind: "pgn", text: "1. e4 e5", game_index: 0 }),
    );
  });

  it("asks which game to review when the PGN holds several", async () => {
    const { user, onStart } = setup({
      pgnGames: [
        { index: 0, white: "Ann", black: "Bob", result: "1-0", event: "?", date: "?", moves: 40 },
        { index: 1, white: "Cy", black: "Di", result: "0-1", event: "?", date: "?", moves: 21 },
      ],
    });
    await user.type(screen.getByLabelText("PGN text"), "two games");
    await user.click(screen.getByRole("button", { name: "Review" }));

    expect(await screen.findByText(/contains 2 games/)).toBeInTheDocument();
    expect(onStart).not.toHaveBeenCalled();
    await user.click(screen.getByLabelText(/Cy vs Di/));
    await user.click(screen.getByRole("button", { name: "Review selected game" }));
    expect(onStart).toHaveBeenCalledWith({ kind: "pgn", text: "two games", game_index: 1 });
  });

  it("forgets the game choices when the text is edited", async () => {
    const { user } = setup({
      pgnGames: [
        { index: 0, white: "Ann", black: "Bob", result: "1-0", event: "?", date: "?", moves: 40 },
        { index: 1, white: "Cy", black: "Di", result: "0-1", event: "?", date: "?", moves: 21 },
      ],
    });
    await user.type(screen.getByLabelText("PGN text"), "x");
    await user.click(screen.getByRole("button", { name: "Review" }));
    await screen.findByText(/contains 2 games/);
    await user.type(screen.getByLabelText("PGN text"), "y");
    expect(screen.queryByText(/contains 2 games/)).not.toBeInTheDocument();
  });

  it("shows why a PGN cannot be used", async () => {
    const api = createFakeApi();
    api.parsePgnGames = async () => {
      throw "no game found in the PGN";
    };
    const onStart = vi.fn();
    render(<HomeScreen api={api} onStart={onStart} onOpen={() => {}} />);
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("PGN text"), "garbage");
    await user.click(screen.getByRole("button", { name: "Review" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("no game found in the PGN");
    expect(onStart).not.toHaveBeenCalled();
  });

  it("fills the box from a PGN file", async () => {
    const { user, api } = setup({ pickedPath: "C:/games/mine.pgn", fileText: "1. d4 d5" });
    await user.click(screen.getByRole("button", { name: "Open PGN file…" }));
    await waitFor(() => expect(screen.getByLabelText("PGN text")).toHaveValue("1. d4 d5"));
    expect(api.calls).toContainEqual(["readPgnFile", "C:/games/mine.pgn"]);
  });

  it("does nothing when the file picker is cancelled", async () => {
    const { user, api } = setup({ pickedPath: null });
    await user.click(screen.getByRole("button", { name: "Open PGN file…" }));
    expect(screen.getByLabelText("PGN text")).toHaveValue("");
    expect(api.calls.some((c) => c[0] === "readPgnFile")).toBe(false);
  });

  it("lists recent games and opens one", async () => {
    const { user, onOpen } = setup();
    const game = await screen.findByRole("button", { name: /^Paul Morphy vs Duke/ });
    expect(game).toHaveTextContent("1-0");
    await user.click(game);
    expect(onOpen).toHaveBeenCalledWith(operaGame.summary.id);
  });

  it("deletes a recent game and refreshes the list", async () => {
    const { user, api } = setup();
    await user.click(await screen.findByRole("button", { name: "Delete A vs B" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: /Delete A vs B/ })).not.toBeInTheDocument());
    expect(api.calls).toContainEqual(["deleteGame", foolsMate.summary.id]);
  });

  it("says so when there is no history", async () => {
    setup({ games: [] });
    expect(await screen.findByText("Reviewed games will appear here.")).toBeInTheDocument();
  });

  it("shows a notice passed down from a failed start", () => {
    const api = createFakeApi();
    render(<HomeScreen api={api} onStart={() => {}} onOpen={() => {}} notice="Stockfish was not found" />);
    expect(screen.getByRole("alert")).toHaveTextContent("Stockfish was not found");
  });

  it("says nothing about Stockfish when it works", async () => {
    const { api } = setup();
    await waitFor(() => expect(api.calls.some((c) => c[0] === "checkEngine")).toBe(true));
    expect(screen.queryByText(/Stockfish was not found/)).not.toBeInTheDocument();
  });

  it("tells the user when Stockfish is missing and leads them to Settings", async () => {
    const { user, onOpenSettings } = setup({
      engine: { found: false, name: null, error: "Stockfish was not found" },
    });
    expect(await screen.findByText(/Stockfish was not found, so games cannot be reviewed yet/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Get Stockfish in Settings" }));
    expect(onOpenSettings).toHaveBeenCalledOnce();
  });

  it("stays quiet when the engine check itself fails", async () => {
    const api = createFakeApi({ games: [operaGame] });
    api.checkEngine = async () => {
      throw "boom";
    };
    render(<HomeScreen api={api} onStart={vi.fn()} onOpen={vi.fn()} />);
    await screen.findByRole("heading", { name: "Review a game" });
    expect(screen.queryByText(/Stockfish was not found/)).not.toBeInTheDocument();
  });
});
