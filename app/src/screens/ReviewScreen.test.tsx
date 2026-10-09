import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { foolsMate, operaGame } from "../fixtures";
import { applyJobEvent, fromStored, startLive } from "../lib/reviewData";
import { lastBoardOptions } from "../test-utils/boardStub";
import { ReviewScreen } from "./ReviewScreen";

const complete = fromStored(foolsMate);

function setup(props: Partial<React.ComponentProps<typeof ReviewScreen>> = {}) {
  const onCancel = vi.fn();
  const onBack = vi.fn();
  render(
    <ReviewScreen data={complete} status="complete" onCancel={onCancel} onBack={onBack} {...props} />,
  );
  return { onCancel, onBack, user: userEvent.setup() };
}

describe("ReviewScreen", () => {
  it("opens on the starting position of a finished review", () => {
    setup();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("A vs B");
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", foolsMate.game.positions[0]);
    expect(screen.getByText("The starting position.")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("steps through the game and explains each move", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Last position" }));
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", foolsMate.game.positions[4]);
    await user.click(screen.getByRole("button", { name: "Previous move" }));
    expect(screen.getByText(/^g4 was a blunder; Nc3 was better\./)).toBeInTheDocument();
    expect(lastBoardOptions().arrows).toHaveLength(1); // the move it should have played
  });

  it("jumps to a move chosen in the list or the graph", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "g4, Blunder" }));
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", foolsMate.game.positions[3]);
    fireEvent.click(screen.getByRole("button", { name: "Go to move 1" }));
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", foolsMate.game.positions[1]);
  });

  it("follows the keyboard", () => {
    setup();
    fireEvent.keyDown(window, { key: "ArrowRight" });
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", foolsMate.game.positions[2]);
    fireEvent.keyDown(window, { key: "End" });
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", foolsMate.game.positions[4]);
  });

  it("flips the board and can hide the best-move arrow", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Flip board" }));
    expect(lastBoardOptions().boardOrientation).toBe("black");

    await user.click(screen.getByRole("button", { name: "g4, Blunder" }));
    expect(lastBoardOptions().arrows).toHaveLength(1);
    await user.click(screen.getByLabelText("Best move"));
    expect(lastBoardOptions().arrows).toHaveLength(0);
  });

  it("shows the evaluation of the position being viewed", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Last position" }));
    expect(screen.getByRole("img", { name: "Evaluation 0-1 #" })).toBeInTheDocument();
  });

  it("goes back", async () => {
    const { user, onBack } = setup();
    await user.click(screen.getByRole("button", { name: "← Back" }));
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("shows progress and a cancel button while the engine is working", async () => {
    let data = startLive(operaGame.game);
    data = applyJobEvent(data, {
      kind: "analysed",
      job: 1,
      index: 0,
      total: data.evals.length,
      eval: operaGame.review.evals[0],
    });
    const { user, onCancel } = setup({ data, status: "running" });
    expect(screen.getByRole("status")).toHaveTextContent("Analysing position 2 of 34");
    expect(screen.getByLabelText("Analysis progress")).toHaveAttribute("value", "1");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();
    // The move list is already there, just not classified yet.
    expect(screen.getByRole("button", { name: "e4" })).toBeInTheDocument();
  });

  it("says so when the review failed, and keeps what was analysed", () => {
    setup({ status: "failed", message: "the engine did not answer within 120s" });
    expect(screen.getByRole("alert")).toHaveTextContent("The review stopped: the engine did not answer within 120s");
    expect(within(screen.getByRole("list", { name: "Moves" })).getAllByRole("listitem").length).toBeGreaterThan(0);
  });

  it("says so when the review was cancelled", () => {
    setup({ status: "cancelled" });
    expect(screen.getByRole("status")).toHaveTextContent("Review cancelled");
  });

  it("starts a new review at the start rather than keeping an old position", () => {
    const { rerender } = render(
      <ReviewScreen data={complete} status="complete" onCancel={() => {}} onBack={() => {}} />,
    );
    fireEvent.keyDown(window, { key: "End" });
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", foolsMate.game.positions[4]);
    // Same screen, different game: the parent remounts it with a key (see App); simulate that.
    rerender(
      <ReviewScreen key="other" data={fromStored(operaGame)} status="complete" onCancel={() => {}} onBack={() => {}} />,
    );
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", operaGame.game.positions[0]);
  });
});
