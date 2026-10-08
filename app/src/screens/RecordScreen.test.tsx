import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { lastBoardOptions } from "../test-utils/boardStub";
import { RecordScreen } from "./RecordScreen";

const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

function setup() {
  const onReview = vi.fn();
  const onCancel = vi.fn();
  render(<RecordScreen onReview={onReview} onCancel={onCancel} />);
  return { onReview, onCancel, user: userEvent.setup() };
}

/** Drops a piece on the (stubbed) board, as dragging would. Returns whether it was accepted. */
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

function playAll(...moves: string[]) {
  for (const move of moves) {
    expect(drop(move.slice(0, 2), move.slice(2, 4)), `move ${move}`).toBe(true);
  }
}

describe("RecordScreen", () => {
  it("starts with an empty game and nothing to review", () => {
    setup();
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", START);
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
    expect(screen.getByRole("button", { name: "Review this game" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Take back" })).toBeDisabled();
    expect(lastBoardOptions().allowDragging).toBe(true);
  });

  it("records the moves played on the board and shows them in the list", () => {
    setup();
    playAll("e2e4", "e7e5");
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
    expect(screen.getByRole("button", { name: "e4" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "e5" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Review this game" })).toBeEnabled();
    expect(screen.getByTestId("chessboard")).toHaveAttribute(
      "data-fen",
      expect.stringContaining("4p3"),
    );
  });

  it("refuses an illegal move and records nothing", () => {
    setup();
    expect(drop("e2", "e5")).toBe(false);
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
    expect(screen.queryByRole("button", { name: "e5" })).not.toBeInTheDocument();
  });

  it("takes the last move back", async () => {
    const { user } = setup();
    playAll("e2e4", "e7e5");
    await user.click(screen.getByRole("button", { name: "Take back" }));
    expect(screen.queryByRole("button", { name: "e5" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Black to move");
  });

  it("asks which piece to promote to, then records the move", async () => {
    const { user } = setup();
    playAll("h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5");

    // Dropping on the last rank is held back until a piece is chosen.
    expect(drop("g7", "g8")).toBe(false);
    expect(screen.getByRole("group", { name: "Promote to" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "g8=N" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Knight" }));
    expect(screen.queryByRole("group", { name: "Promote to" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^g8=N/ })).toBeInTheDocument();
  });

  it("can back out of a promotion", async () => {
    const { user } = setup();
    playAll("h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5");
    drop("g7", "g8");
    await user.click(screen.getByRole("button", { name: "Cancel promotion" }));
    expect(screen.queryByRole("group", { name: "Promote to" })).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("White to move");
  });

  it("ends the game on checkmate and stops accepting moves", () => {
    setup();
    playAll("f2f3", "e7e5", "g2g4", "d8h4");
    expect(screen.getByRole("status")).toHaveTextContent("Checkmate: 0-1");
    expect(drop("a2", "a3")).toBe(false);
    expect(screen.getByLabelText("Result")).toBeDisabled();
    expect(screen.getByLabelText("Result")).toHaveValue("0-1");
  });

  it("says why a game was drawn", () => {
    setup();
    // Sam Loyd's ten-move stalemate.
    playAll(
      "e2e3", "a7a5", "d1h5", "a8a6", "h5a5", "h7h5", "h2h4", "a6h6", "a5c7", "f7f6",
      "c7d7", "e8f7", "d7b7", "d8d3", "b7b8", "d3h7", "b8c8", "f7g6", "c8e6",
    );
    expect(screen.getByRole("status")).toHaveTextContent("Draw by stalemate");
    expect(screen.getByLabelText("Result")).toHaveValue("1/2-1/2");
  });

  it("keeps accepting moves after a position has repeated three times", () => {
    setup();
    playAll("g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1", "f6g8", "e2e4");
    expect(screen.getByRole("status")).toHaveTextContent("Black to move");
  });

  it("sends the moves, names and result to be reviewed", async () => {
    const { user, onReview } = setup();
    await user.type(screen.getByLabelText("White"), "Me");
    await user.type(screen.getByLabelText("Black"), "My friend");
    playAll("e2e4", "e7e5");
    await user.selectOptions(screen.getByLabelText("Result"), "1-0");
    await user.click(screen.getByRole("button", { name: "Review this game" }));

    expect(onReview).toHaveBeenCalledOnce();
    const source = onReview.mock.calls[0][0];
    expect(source).toMatchObject({ kind: "moves", start_fen: null, uci_moves: ["e2e4", "e7e5"] });
    expect(source.headers).toMatchObject({ White: "Me", Black: "My friend", Result: "1-0" });
  });

  it("starts over", async () => {
    const { user } = setup();
    playAll("e2e4");
    await user.click(screen.getByRole("button", { name: "New game" }));
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", START);
    expect(screen.queryByRole("button", { name: "e4" })).not.toBeInTheDocument();
  });

  it("flips the board", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Flip board" }));
    expect(lastBoardOptions().boardOrientation).toBe("black");
  });

  it("highlights the last move on the board", () => {
    setup();
    playAll("e2e4");
    expect(Object.keys(lastBoardOptions().squareStyles ?? {}).sort()).toEqual(["e2", "e4"]);
  });

  it("can be cancelled", async () => {
    const { user, onCancel } = setup();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();
  });
});
