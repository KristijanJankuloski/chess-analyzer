import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { foolsMate } from "../fixtures";
import { fromStored, startLive } from "../lib/reviewData";
import { SummaryPanel } from "./SummaryPanel";

describe("SummaryPanel", () => {
  it("shows both players with their accuracy and the opening", () => {
    render(<SummaryPanel data={fromStored(foolsMate)} />);
    expect(screen.getByLabelText("White accuracy")).toHaveTextContent("39.9");
    expect(screen.getByLabelText("Black accuracy")).toHaveTextContent("100.0");
    expect(screen.getByText("A")).toBeInTheDocument();
    expect(screen.getByText("B")).toBeInTheDocument();
    expect(screen.getByText("A00 Barnes Opening: Fool's Mate")).toBeInTheDocument();
  });

  it("counts each side's moves by class, leaving out classes nobody played", () => {
    render(<SummaryPanel data={fromStored(foolsMate)} />);
    const rows = screen.getAllByRole("row").map((r) => r.textContent);
    expect(rows).toEqual(["1Book2", "1Blunder0"]);
  });

  it("shows dashes and a placeholder while the review is still running", () => {
    render(<SummaryPanel data={startLive(foolsMate.game)} />);
    expect(screen.getByLabelText("White accuracy")).toHaveTextContent("–");
    expect(screen.getByText("Naming the opening once the review is done…")).toBeInTheDocument();
    expect(screen.queryAllByRole("row")).toHaveLength(0);
  });

  it("says when a finished game is not in the opening book", () => {
    const data = { ...fromStored(foolsMate), opening: null };
    render(<SummaryPanel data={data} />);
    expect(screen.getByText("Opening not in the book")).toBeInTheDocument();
  });
});
