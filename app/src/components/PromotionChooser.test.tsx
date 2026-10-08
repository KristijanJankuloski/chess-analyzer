import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { PromotionChooser } from "./PromotionChooser";

describe("PromotionChooser", () => {
  it("offers the four pieces and reports the one chosen", async () => {
    const onChoose = vi.fn();
    render(<PromotionChooser onChoose={onChoose} onCancel={() => {}} />);
    expect(screen.getByRole("group", { name: "Promote to" })).toBeInTheDocument();
    for (const name of ["Queen", "Rook", "Bishop", "Knight"]) {
      expect(screen.getByRole("button", { name })).toBeInTheDocument();
    }
    await userEvent.click(screen.getByRole("button", { name: "Knight" }));
    expect(onChoose).toHaveBeenCalledWith("n");
    await userEvent.click(screen.getByRole("button", { name: "Queen" }));
    expect(onChoose).toHaveBeenLastCalledWith("q");
  });

  it("can be cancelled", async () => {
    const onCancel = vi.fn();
    render(<PromotionChooser onChoose={() => {}} onCancel={onCancel} />);
    await userEvent.click(screen.getByRole("button", { name: "Cancel promotion" }));
    expect(onCancel).toHaveBeenCalled();
  });
});
