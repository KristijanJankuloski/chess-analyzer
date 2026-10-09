import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import type { LiveEvent } from "../generated/LiveEvent";
import { type LiveState, applyLiveEvent, initialLive, startRevision } from "../lib/live";
import { type RecordDraft, emptyDraft } from "../lib/record";
import { lastBoardOptions } from "../test-utils/boardStub";
import { cp, line, move, position, review } from "../test-utils/liveEvents";
import { LiveScreen } from "./LiveScreen";

const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/** The state after the stream of `events`, for a game of `moves`. */
function liveState(moves: string[], events: LiveEvent[]): LiveState {
  return events.reduce(applyLiveEvent, startRevision(initialLive, 1, moves));
}

/** 1. e4 e5: the engine likes e4, thinks e5 a (provisional) mistake and would play Nf3. */
const MOVES = ["e2e4", "e7e5"];
const ANALYSED = liveState(MOVES, [
  position(1, 0, "e2e4", cp(30)),
  position(1, 1, "e7e5", cp(25)),
  {
    kind: "position",
    revision: 1,
    index: 2,
    depth: 22,
    lines: [line("g1f3", cp(31), ["Nf3", "Nc6", "Bb5"])],
  },
  move(1, review(1, "e2e4", "best", "e2e4"), false),
  move(1, review(2, "e7e5", "mistake", "c7c5"), true),
]);

function Harness({
  live = initialLive,
  initial = emptyDraft,
  onReview = () => {},
  onRestart = () => {},
  onBack = () => {},
}: {
  live?: LiveState;
  initial?: RecordDraft;
  onReview?: (source: unknown) => void;
  onRestart?: () => void;
  onBack?: () => void;
}) {
  const [draft, setDraft] = useState(initial);
  return (
    <LiveScreen
      draft={draft}
      onDraftChange={setDraft}
      live={live}
      onRestart={onRestart}
      onReview={onReview}
      onBack={onBack}
    />
  );
}

const withMoves = (...uciMoves: string[]): RecordDraft => ({ ...emptyDraft, recording: { uciMoves } });

function drop(from: string, to: string): boolean {
  let accepted = false;
  act(() => {
    accepted = lastBoardOptions().onPieceDrop!({
      piece: { isSparePiece: false, position: from, pieceType: "wP" },
      sourceSquare: from,
      targetSquare: to,
    });
  });
  return accepted;
}

describe("LiveScreen commentary", () => {
  const WRITTEN = "e5 is a mistake; c5 was better. After Nf3, the pawn on e5 is attacked and short of protection.";
  const withCommentary = (provisional: boolean) =>
    liveState(MOVES, [
      position(1, 0, "e2e4", cp(30)),
      position(1, 1, "e7e5", cp(25)),
      position(1, 2, "g1f3", cp(31)),
      move(1, review(1, "e2e4", "best", "e2e4"), false),
      move(1, review(2, "e7e5", "mistake", "c7c5", 90, WRITTEN), provisional),
    ]);

  it("shows the commentary for the latest move, with no extra note when it is provisional", () => {
    for (const provisional of [true, false]) {
      const { unmount } = render(<Harness live={withCommentary(provisional)} initial={withMoves(...MOVES)} />);
      expect(screen.getByText(/^e5 is a mistake; c5 was better\./)).toBeInTheDocument();
      expect(screen.queryByText(/may change/)).not.toBeInTheDocument();
      unmount();
    }
  });

  it("falls back to the short sentence while a move has no commentary", () => {
    render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
    expect(screen.getByText(/^e7e5 is a mistake\./)).toBeInTheDocument();
  });

  it("says a move is being analysed before the engine has classified it, and shows nothing at the start", () => {
    const { unmount } = render(<Harness initial={withMoves("e2e4")} />);
    expect(screen.getByText("Analysing this move…")).toBeInTheDocument();
    unmount();
    render(<Harness />);
    expect(screen.queryByText("Analysing this move…")).not.toBeInTheDocument();
    expect(screen.queryByText("The starting position.")).not.toBeInTheDocument();
  });
});

describe("LiveScreen", () => {
  it("starts with an empty board, nothing to review and the engine still to answer", () => {
    render(<Harness />);
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", START);
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
    expect(screen.getByRole("button", { name: "Review this game" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Take back" })).toBeDisabled();
    expect(screen.getByText("Waiting for the engine…")).toBeInTheDocument();
    expect(lastBoardOptions().allowDragging).toBe(true);
  });

  describe("entering moves", () => {
    it("takes a move played on the board", () => {
      render(<Harness />);
      expect(drop("e2", "e4")).toBe(true);
      expect(screen.getByRole("button", { name: "e4" })).toBeInTheDocument();
      expect(screen.getByRole("status")).toHaveTextContent("Black to move");
    });

    it("refuses an illegal move on the board", () => {
      render(<Harness />);
      expect(drop("e2", "e5")).toBe(false);
      expect(screen.queryByRole("button", { name: "e5" })).not.toBeInTheDocument();
    });

    it("takes a move typed in algebraic notation and clears the box for the next one", async () => {
      const user = userEvent.setup();
      render(<Harness />);
      const box = screen.getByLabelText("Move");
      await user.type(box, "e4{Enter}");
      expect(screen.getByRole("button", { name: "e4" })).toBeInTheDocument();
      expect(box).toHaveValue("");
      await user.type(box, "nc6{Enter}");
      expect(screen.getByRole("button", { name: "Nc6" })).toBeInTheDocument();
    });

    it("says why a typed move was refused and keeps the text to correct", async () => {
      const user = userEvent.setup();
      render(<Harness />);
      const box = screen.getByLabelText("Move");
      await user.type(box, "Nf6{Enter}");
      expect(screen.getByRole("alert")).toHaveTextContent("Illegal move: Nf6");
      expect(box).toHaveValue("Nf6");
      expect(screen.queryByRole("button", { name: "Nf6" })).not.toBeInTheDocument();

      await user.clear(box);
      await user.type(box, "Nf3");
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    });

    it("asks for a piece when a pawn reaches the last rank", async () => {
      const user = userEvent.setup();
      render(
        <Harness
          initial={withMoves("h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5")}
        />,
      );
      expect(drop("g7", "g8")).toBe(false);
      await user.click(screen.getByRole("button", { name: "Knight" }));
      expect(screen.getByRole("button", { name: "g8=N" })).toBeInTheDocument();
    });

    it("takes the last move back", async () => {
      const user = userEvent.setup();
      render(<Harness initial={withMoves(...MOVES)} />);
      await user.click(screen.getByRole("button", { name: "Take back" }));
      expect(screen.queryByRole("button", { name: "e5" })).not.toBeInTheDocument();
      expect(screen.getByRole("status")).toHaveTextContent("Black to move");
    });

    it("asks before throwing a game away, then starts a new one", async () => {
      const user = userEvent.setup();
      render(<Harness initial={withMoves(...MOVES)} />);
      await user.click(screen.getByRole("button", { name: "New game" }));
      expect(screen.getByRole("group", { name: "Discard this game?" })).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Keep playing" }));
      expect(screen.getByRole("button", { name: "e5" })).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "New game" }));
      await user.click(screen.getByRole("button", { name: "Discard game" }));
      expect(screen.queryByRole("button", { name: "e4" })).not.toBeInTheDocument();
      expect(screen.getByRole("status")).toHaveTextContent("White to move");
    });

    it("stops taking moves once the game has ended on the board", () => {
      render(<Harness initial={withMoves("f2f3", "e7e5", "g2g4", "d8h4")} />);
      expect(screen.getByRole("status")).toHaveTextContent("Checkmate: 0-1");
      expect(lastBoardOptions().allowDragging).toBe(false);
      expect(screen.getByLabelText("Move")).toBeDisabled();
      expect(screen.getByLabelText("Result")).toHaveValue("0-1");
    });
  });

  describe("the live review", () => {
    it("shows the evaluation, the engine's lines and the depth it has reached", () => {
      render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
      expect(screen.getByRole("img", { name: "Evaluation +0.31" })).toBeInTheDocument();
      expect(screen.getByText("depth 22")).toBeInTheDocument();
      expect(screen.getByText("2. Nf3 Nc6 3. Bb5")).toBeInTheDocument();
    });

    it("draws the engine's best move and what should have been played instead", () => {
      render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
      const arrows = lastBoardOptions().arrows ?? [];
      expect(arrows.map((a) => `${a.startSquare}${a.endSquare}`).sort()).toEqual(["c7c5", "g1f3"]);
    });

    it("badges the last move, marked provisional while it may still change", () => {
      render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
      const { container } = render(
        lastBoardOptions().squareRenderer!({ piece: null, square: "e5", children: null }),
      );
      const badge = container.querySelector(".class-badge");
      expect(badge).toHaveAttribute("title", "Mistake (provisional)");
      expect(screen.getByRole("button", { name: "e5, Mistake, provisional" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "e4, Best" })).toBeInTheDocument();
    });

    it("shows each side's accuracy so far", () => {
      render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
      expect(screen.getByLabelText("White accuracy")).toHaveTextContent("90.0");
      expect(screen.getByLabelText("Black accuracy")).toHaveTextContent("90.0");
    });

    it("hides the engine's current best move on request but keeps the arrow for the move just played", async () => {
      const user = userEvent.setup();
      render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
      expect(lastBoardOptions().arrows).toHaveLength(2);

      await user.click(screen.getByRole("checkbox", { name: "Current best move" }));
      const arrows = lastBoardOptions().arrows ?? [];
      expect(arrows.map((a) => `${a.startSquare}${a.endSquare}`)).toEqual(["c7c5"]);

      await user.click(screen.getByRole("checkbox", { name: "Current best move" }));
      expect(lastBoardOptions().arrows).toHaveLength(2);
    });

    it("shows nothing for moves the engine has not looked at yet", () => {
      render(<Harness live={initialLive} initial={withMoves(...MOVES)} />);
      expect(screen.getByRole("img", { name: "No evaluation yet" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "e5" })).toBeInTheDocument();
      expect(lastBoardOptions().arrows).toEqual([]);
    });
  });

  describe("looking back over the game", () => {
    it("shows the earlier position with what the engine said then, and refuses new moves", async () => {
      const user = userEvent.setup();
      render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
      await user.click(screen.getByRole("button", { name: "Previous move" }));

      expect(screen.getByTestId("chessboard")).toHaveAttribute(
        "data-fen",
        expect.stringContaining("4P3"),
      );
      expect(screen.getByText("depth 20")).toBeInTheDocument();
      expect(lastBoardOptions().allowDragging).toBe(false);
      expect(screen.getByLabelText("Move")).toBeDisabled();
      expect(screen.getByText("Go to the latest move to enter more.")).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Last position" }));
      expect(lastBoardOptions().allowDragging).toBe(true);
      expect(screen.getByLabelText("Move")).toBeEnabled();
    });

    it("can jump to a move from the list", async () => {
      const user = userEvent.setup();
      render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
      await user.click(screen.getByRole("button", { name: "e4, Best" }));
      expect(screen.getByText("depth 20")).toBeInTheDocument();
    });

    it("returns to the end when a move is taken back", async () => {
      const user = userEvent.setup();
      render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
      await user.click(screen.getByRole("button", { name: "First position" }));
      await user.click(screen.getByRole("button", { name: "Take back" }));
      expect(lastBoardOptions().allowDragging).toBe(true);
      expect(screen.getByRole("status")).toHaveTextContent("Black to move");
    });
  });

  describe("when something goes wrong", () => {
    it("explains why the analysis stopped and offers to restart it", async () => {
      const user = userEvent.setup();
      const onRestart = vi.fn();
      render(
        <Harness
          live={{ ...ANALYSED, error: "Stockfish was not found" }}
          initial={withMoves(...MOVES)}
          onRestart={onRestart}
        />,
      );
      expect(screen.getByRole("alert")).toHaveTextContent("Stockfish was not found");
      await user.click(screen.getByRole("button", { name: "Restart analysis" }));
      expect(onRestart).toHaveBeenCalled();
    });

    it("keeps taking moves while the engine is down", async () => {
      const user = userEvent.setup();
      render(<Harness live={{ ...initialLive, error: "no engine" }} />);
      await user.type(screen.getByLabelText("Move"), "e4{Enter}");
      expect(screen.getByRole("button", { name: "e4" })).toBeInTheDocument();
    });
  });

  describe("finishing", () => {
    it("sends the game, with its players, to be reviewed", async () => {
      const user = userEvent.setup();
      const onReview = vi.fn();
      render(<Harness initial={withMoves("e2e4", "e7e5")} onReview={onReview} />);
      await user.type(screen.getByLabelText("White"), "Magnus");
      await user.type(screen.getByLabelText("Black"), "Hikaru");
      await user.selectOptions(screen.getByLabelText("Result"), "1/2-1/2");
      await user.click(screen.getByRole("button", { name: "Review this game" }));

      const source = onReview.mock.calls[0][0];
      expect(source).toMatchObject({ kind: "moves", uci_moves: ["e2e4", "e7e5"] });
      expect(source.headers).toMatchObject({ White: "Magnus", Black: "Hikaru", Result: "1/2-1/2" });
    });

    it("goes back without losing the game", async () => {
      const user = userEvent.setup();
      const onBack = vi.fn();
      render(<Harness initial={withMoves("e2e4")} onBack={onBack} />);
      await user.click(screen.getByRole("button", { name: "← Back" }));
      expect(onBack).toHaveBeenCalled();
    });
  });
});
