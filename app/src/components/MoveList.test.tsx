import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { foolsMate } from "../fixtures";
import { fromStored, startLive } from "../lib/reviewData";
import { moveRows } from "../lib/rows";
import { MoveList } from "./MoveList";

describe("MoveList", () => {
  const rows = moveRows(fromStored(foolsMate));

  it("lists the moves in pairs under their numbers, each with its class", () => {
    render(<MoveList rows={rows} selectedPly={0} onSelect={() => {}} />);
    const items = within(screen.getByRole("list", { name: "Moves" })).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("1.");
    expect(screen.getByRole("button", { name: "g4, Blunder" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "f3, Book" })).toBeInTheDocument();
  });

  it("selects the move that was clicked", () => {
    const onSelect = vi.fn();
    render(<MoveList rows={rows} selectedPly={0} onSelect={onSelect} />);
    fireEvent.click(screen.getByRole("button", { name: "Qh4#, Book" }));
    expect(onSelect).toHaveBeenCalledWith(4);
  });

  it("highlights the selected move for assistive technology too", () => {
    render(<MoveList rows={rows} selectedPly={3} onSelect={() => {}} />);
    expect(screen.getByRole("button", { name: "g4, Blunder" })).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: "f3, Book" })).not.toHaveAttribute("aria-current");
  });

  it("shows moves that are not classified yet without a mark", () => {
    const live = moveRows(startLive(foolsMate.game));
    render(<MoveList rows={live} selectedPly={0} onSelect={() => {}} />);
    expect(screen.getByRole("button", { name: "f3" })).toBeInTheDocument();
  });

  it("copes with a final row that has no Black move", () => {
    const odd = moveRows({ ...fromStored(foolsMate), game: { ...foolsMate.game, moves: foolsMate.game.moves.slice(0, 3), positions: foolsMate.game.positions.slice(0, 4) }, moves: foolsMate.review.moves.slice(0, 3) });
    render(<MoveList rows={odd} selectedPly={0} onSelect={() => {}} />);
    const last = screen.getAllByRole("listitem")[1];
    expect(last).toHaveTextContent("…");
  });
});

describe("MoveList in a live game", () => {
  const data = fromStored(foolsMate);

  it("marks a class that may still change and says so to assistive technology", () => {
    const rows = moveRows(data, [false, false, true, false]);
    render(<MoveList rows={rows} selectedPly={0} onSelect={() => {}} />);
    const provisional = screen.getByRole("button", { name: "g4, Blunder, provisional" });
    expect(provisional.querySelector(".move__mark")).toHaveClass("move__mark--provisional");
    const settled = screen.getByRole("button", { name: "f3, Book" });
    expect(settled.querySelector(".move__mark")).not.toHaveClass("move__mark--provisional");
  });
});
