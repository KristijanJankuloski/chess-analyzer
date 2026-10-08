import { act, render, screen } from "@testing-library/react";
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

  describe("click to move", () => {
    const pawn = { pieceType: "wP" };
    const click = (square: string, piece: { pieceType: string } | null) =>
      act(() => lastBoardOptions().onSquareClick!({ piece, square }));

    it("moves a piece with two clicks", () => {
      const onMove = vi.fn().mockReturnValue(true);
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      expect(Object.keys(lastBoardOptions().squareStyles ?? {})).toEqual(["e2"]);
      click("e4", null);
      expect(onMove).toHaveBeenCalledWith("e2", "e4");
      expect(lastBoardOptions().squareStyles).toEqual({});
    });

    it("clears the selection when the same square is clicked again", () => {
      const onMove = vi.fn();
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      click("e2", pawn);
      expect(lastBoardOptions().squareStyles).toEqual({});
      expect(onMove).not.toHaveBeenCalled();
    });

    it("selects a different piece when the move is refused and the square holds a piece", () => {
      const onMove = vi.fn().mockReturnValue(false);
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      click("d2", pawn);
      expect(onMove).toHaveBeenCalledWith("e2", "d2");
      expect(Object.keys(lastBoardOptions().squareStyles ?? {})).toEqual(["d2"]);
    });

    it("forgets the selection when a refused move lands on an empty square", () => {
      const onMove = vi.fn().mockReturnValue(false);
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      click("e5", null);
      expect(lastBoardOptions().squareStyles).toEqual({});
    });

    it("ignores clicks on empty squares when nothing is selected", () => {
      const onMove = vi.fn();
      render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e5", null);
      expect(onMove).not.toHaveBeenCalled();
      expect(lastBoardOptions().squareStyles).toEqual({});
    });

    it("does nothing on a board that is not playable", () => {
      render(<BoardPanel fen={FEN} orientation="white" />);
      click("e2", pawn);
      expect(lastBoardOptions().squareStyles).toEqual({});
    });

    it("drops the selection when the position changes", () => {
      const onMove = vi.fn().mockReturnValue(true);
      const { rerender } = render(<BoardPanel fen={FEN} orientation="white" onMove={onMove} />);
      click("e2", pawn);
      rerender(<BoardPanel fen={FEN.replace(" w ", " b ")} orientation="white" onMove={onMove} />);
      expect(lastBoardOptions().squareStyles).toEqual({});
    });
  });
});

describe("BoardPanel in a live game", () => {
  it("draws the move that should have been played in red beside the engine's best move in green", () => {
    render(
      <BoardPanel
        fen={FEN}
        orientation="white"
        bestArrow={{ from: "g1", to: "f3" }}
        missArrow={{ from: "d2", to: "d4" }}
      />,
    );
    const arrows = lastBoardOptions().arrows ?? [];
    expect(arrows).toHaveLength(2);
    const red = arrows.find((a) => a.startSquare === "d2");
    const green = arrows.find((a) => a.startSquare === "g1");
    expect(red?.endSquare).toBe("d4");
    expect(green?.endSquare).toBe("f3");
    expect(red?.color).not.toBe(green?.color);
  });

  it("draws a red arrow on its own", () => {
    render(<BoardPanel fen={FEN} orientation="white" missArrow={{ from: "d2", to: "d4" }} />);
    expect(lastBoardOptions().arrows).toEqual([
      expect.objectContaining({ startSquare: "d2", endSquare: "d4" }),
    ]);
  });

  it("marks a badge that may still change", () => {
    render(
      <BoardPanel
        fen={FEN}
        orientation="white"
        badge={{ square: "g4", cls: "blunder", provisional: true }}
      />,
    );
    const { container } = render(
      lastBoardOptions().squareRenderer!({ piece: null, square: "g4", children: null }),
    );
    const badge = container.querySelector(".class-badge");
    expect(badge).toHaveClass("class-badge--provisional");
    expect(badge).toHaveAttribute("title", "Blunder (provisional)");
  });

  it("does not mark a settled badge", () => {
    render(
      <BoardPanel
        fen={FEN}
        orientation="white"
        badge={{ square: "g4", cls: "blunder", provisional: false }}
      />,
    );
    const { container } = render(
      lastBoardOptions().squareRenderer!({ piece: null, square: "g4", children: null }),
    );
    expect(container.querySelector(".class-badge")).not.toHaveClass("class-badge--provisional");
    expect(container.querySelector(".class-badge")).toHaveAttribute("title", "Blunder");
  });
});
