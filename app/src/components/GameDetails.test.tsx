import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { GameDetails } from "./GameDetails";

const open = { over: false, result: "*" as const };

describe("GameDetails", () => {
  it("shows the names and the result chosen so far", () => {
    render(
      <GameDetails idPrefix="x" white="Me" black="You" result="1-0" ended={open} onChange={() => {}} />,
    );
    expect(screen.getByLabelText("White")).toHaveValue("Me");
    expect(screen.getByLabelText("Black")).toHaveValue("You");
    expect(screen.getByLabelText("Result")).toHaveValue("1-0");
    expect(screen.getByLabelText("White")).toHaveAttribute("id", "x-white");
  });

  it("reports each change as a patch", () => {
    const onChange = vi.fn();
    render(<GameDetails idPrefix="x" white="" black="" result="*" ended={open} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("White"), { target: { value: "Magnus" } });
    expect(onChange).toHaveBeenLastCalledWith({ white: "Magnus" });
    fireEvent.change(screen.getByLabelText("Black"), { target: { value: "Hikaru" } });
    expect(onChange).toHaveBeenLastCalledWith({ black: "Hikaru" });
    fireEvent.change(screen.getByLabelText("Result"), { target: { value: "1/2-1/2" } });
    expect(onChange).toHaveBeenLastCalledWith({ result: "1/2-1/2" });
  });

  it("fixes the result of a game that ended on the board", () => {
    render(
      <GameDetails
        idPrefix="x"
        white=""
        black=""
        result="*"
        ended={{ over: true, result: "0-1" }}
        onChange={() => {}}
      />,
    );
    const result = screen.getByLabelText("Result");
    expect(result).toBeDisabled();
    expect(result).toHaveValue("0-1");
    expect(screen.getAllByRole("option")).toHaveLength(1);
  });
});
