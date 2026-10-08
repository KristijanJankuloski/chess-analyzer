import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { lastBoardOptions } from "../test-utils/boardStub";
import { BoardPanel } from "./BoardPanel";

const FEN = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

describe("BoardPanel", () => {
  it("shows the position from the side it was asked to", () => {
    render(<BoardPanel fen={FEN} orientation="black" />);
    expect(screen.getByTestId("chessboard")).toHaveAttribute("data-fen", FEN);
    expect(lastBoardOptions().boardOrientation).toBe("black");
  });

  it("highlights both squares of the last move", () => {
    render(<BoardPanel fen={FEN} orientation="white" lastMove={{ from: "e2", to: "e4" }} />);
    const styles = lastBoardOptions().squareStyles ?? {};
    expect(Object.keys(styles).sort()).toEqual(["e2", "e4"]);
  });

  it("draws the best-move arrow only when there is one", () => {
    const { rerender } = render(
      <BoardPanel fen={FEN} orientation="white" bestArrow={{ from: "b1", to: "c3" }} />,
    );
    expect(lastBoardOptions().arrows).toEqual([
      expect.objectContaining({ startSquare: "b1", endSquare: "c3" }),
    ]);
    rerender(<BoardPanel fen={FEN} orientation="white" />);
    expect(lastBoardOptions().arrows).toEqual([]);
  });

  it("marks the class of the last move on its square only", () => {
    render(<BoardPanel fen={FEN} orientation="white" badge={{ square: "g4", cls: "blunder" }} />);
    const renderSquare = lastBoardOptions().squareRenderer!;

    const { container: onSquare } = render(renderSquare({ piece: null, square: "g4", children: "♙" }));
    expect(onSquare.querySelector(".class-badge")).toHaveTextContent("??");
    expect(onSquare.querySelector(".class-badge")).toHaveAttribute("title", "Blunder");

    const { container: elsewhere } = render(renderSquare({ piece: null, square: "e4", children: "♙" }));
    expect(elsewhere.querySelector(".class-badge")).toBeNull();
    expect(elsewhere).toHaveTextContent("♙");
  });

  it("is not playable unless given a move handler", () => {
    const { rerender } = render(<BoardPanel fen={FEN} orientation="white" />);
    expect(lastBoardOptions().allowDragging).toBe(false);
    expect(lastBoardOptions().onPieceDrop!({ piece: { isSparePiece: false, position: "e2", pieceType: "wP" }, sourceSquare: "e2", targetSquare: "e4" })).toBe(false);

    const onMove = vi.fn().mockReturnValue(true);
    rerender(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
    expect(lastBoardOptions().allowDragging).toBe(true);
    const dropped = lastBoardOptions().onPieceDrop!({
      piece: { isSparePiece: false, position: "e2", pieceType: "wP" },
      sourceSquare: "e2",
      targetSquare: "e4",
    });
    expect(dropped).toBe(true);
    expect(onMove).toHaveBeenCalledWith("e2", "e4");
  });

  it("rejects a piece dropped off the board", () => {
    const onMove = vi.fn().mockReturnValue(true);
    render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
    const dropped = lastBoardOptions().onPieceDrop!({
      piece: { isSparePiece: false, position: "e2", pieceType: "wP" },
      sourceSquare: "e2",
      targetSquare: null,
    });
    expect(dropped).toBe(false);
    expect(onMove).not.toHaveBeenCalled();
  });
});
