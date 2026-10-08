import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NavControls } from "./NavControls";

function setup(ply: number, last = 10) {
  const props = {
    ply,
    last,
    onSelect: vi.fn(),
    onFlip: vi.fn(),
    showBest: true,
    onToggleBest: vi.fn(),
  };
  render(<NavControls {...props} />);
  return props;
}

describe("NavControls", () => {
  it("steps and jumps with the buttons", () => {
    const props = setup(4);
    fireEvent.click(screen.getByRole("button", { name: "Previous move" }));
    fireEvent.click(screen.getByRole("button", { name: "Next move" }));
    fireEvent.click(screen.getByRole("button", { name: "First position" }));
    fireEvent.click(screen.getByRole("button", { name: "Last position" }));
    expect(props.onSelect.mock.calls.map((c) => c[0])).toEqual([3, 5, 0, 10]);
  });

  it("disables the buttons that cannot go anywhere", () => {
    setup(0);
    expect(screen.getByRole("button", { name: "Previous move" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "First position" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next move" })).toBeEnabled();
  });

  it("disables forward navigation at the end of the game", () => {
    setup(10);
    expect(screen.getByRole("button", { name: "Next move" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Last position" })).toBeDisabled();
  });

  it("responds to the arrow keys, Home and End, staying within the game", () => {
    const props = setup(4);
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    fireEvent.keyDown(window, { key: "ArrowRight" });
    fireEvent.keyDown(window, { key: "Home" });
    fireEvent.keyDown(window, { key: "End" });
    expect(props.onSelect.mock.calls.map((c) => c[0])).toEqual([3, 5, 0, 10]);
  });

  it("never goes past either end with the keyboard", () => {
    const atStart = setup(0);
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    expect(atStart.onSelect).toHaveBeenLastCalledWith(0);
  });

  it("leaves the keys alone while the user is typing", () => {
    const props = setup(4);
    const box = document.createElement("textarea");
    document.body.appendChild(box);
    fireEvent.keyDown(box, { key: "ArrowLeft" });
    expect(props.onSelect).not.toHaveBeenCalled();
    box.remove();
  });

  it("ignores other keys and modified arrow keys", () => {
    const props = setup(4);
    fireEvent.keyDown(window, { key: "a" });
    fireEvent.keyDown(window, { key: "ArrowLeft", ctrlKey: true });
    expect(props.onSelect).not.toHaveBeenCalled();
  });

  it("flips the board and toggles the best-move arrow", () => {
    const props = setup(2);
    fireEvent.click(screen.getByRole("button", { name: "Flip board" }));
    fireEvent.click(screen.getByLabelText("Best move"));
    expect(props.onFlip).toHaveBeenCalledOnce();
    expect(props.onToggleBest).toHaveBeenCalledOnce();
  });

  it("stops listening for keys when it goes away", () => {
    const props = { ply: 4, last: 10, onSelect: vi.fn(), onFlip: vi.fn(), showBest: true, onToggleBest: vi.fn() };
    const { unmount } = render(<NavControls {...props} />);
    unmount();
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    expect(props.onSelect).not.toHaveBeenCalled();
  });
});
