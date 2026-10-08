import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { foolsMate, operaGame } from "../fixtures";
import { startLive } from "../lib/reviewData";
import { EvalGraph } from "./EvalGraph";

const review = foolsMate.review;

describe("EvalGraph", () => {
  it("has a click target for every position", () => {
    render(<EvalGraph evals={review.evals} moves={review.moves} selectedPly={0} onSelect={() => {}} />);
    expect(screen.getAllByRole("button")).toHaveLength(review.evals.length);
    expect(screen.getByRole("button", { name: "Go to the start" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Go to move 3" })).toBeInTheDocument();
  });

  it("jumps to the position that was clicked", () => {
    const onSelect = vi.fn();
    render(<EvalGraph evals={review.evals} moves={review.moves} selectedPly={0} onSelect={onSelect} />);
    fireEvent.click(screen.getByRole("button", { name: "Go to move 3" }));
    expect(onSelect).toHaveBeenCalledWith(3);
  });

  it("puts the cursor at the selected position", () => {
    const { rerender } = render(
      <EvalGraph evals={review.evals} moves={review.moves} selectedPly={0} onSelect={() => {}} />,
    );
    expect(screen.getByTestId("graph-cursor")).toHaveAttribute("x1", "0");
    rerender(<EvalGraph evals={review.evals} moves={review.moves} selectedPly={4} onSelect={() => {}} />);
    expect(screen.getByTestId("graph-cursor")).toHaveAttribute("x1", "300");
  });

  it("marks only the moves that deserve attention", () => {
    const { container } = render(
      <EvalGraph evals={review.evals} moves={review.moves} selectedPly={0} onSelect={() => {}} />,
    );
    const marks = [...container.querySelectorAll("circle title")].map((t) => t.textContent);
    expect(marks).toEqual(["g4 Blunder"]);
  });

  it("marks several moves in a longer game", () => {
    const { container } = render(
      <EvalGraph
        evals={operaGame.review.evals}
        moves={operaGame.review.moves}
        selectedPly={0}
        onSelect={() => {}}
      />,
    );
    expect(container.querySelectorAll("circle").length).toBe(operaGame.review.critical_plies.length);
  });

  it("draws only what has been analysed so far", () => {
    const live = startLive(foolsMate.game);
    const evals = [review.evals[0], review.evals[1], null, null, null];
    const { container } = render(
      <EvalGraph evals={evals} moves={live.moves} selectedPly={0} onSelect={() => {}} />,
    );
    const polygon = container.querySelector("polygon")!;
    // start-of-area, two points, end-of-area
    expect(polygon.getAttribute("points")!.trim().split(" ")).toHaveLength(4);
  });

  it("draws nothing but the background before any analysis", () => {
    const live = startLive(foolsMate.game);
    const { container } = render(
      <EvalGraph evals={live.evals} moves={live.moves} selectedPly={0} onSelect={() => {}} />,
    );
    expect(container.querySelector("polygon")).toBeNull();
  });
});
