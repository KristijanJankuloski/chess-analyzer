import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EvalBar } from "./EvalBar";

describe("EvalBar", () => {
  it("splits the bar by who is winning and labels the evaluation", () => {
    const { container } = render(<EvalBar value={{ kind: "cp", value: 300 }} orientation="white" />);
    expect(screen.getByRole("img", { name: "Evaluation +3.00" })).toBeInTheDocument();
    const white = container.querySelector<HTMLElement>(".eval-bar__white")!;
    const black = container.querySelector<HTMLElement>(".eval-bar__black")!;
    expect(parseFloat(white.style.height)).toBeGreaterThan(70);
    expect(parseFloat(white.style.height) + parseFloat(black.style.height)).toBeCloseTo(100, 6);
  });

  it("is level when nothing is known yet", () => {
    const { container } = render(<EvalBar value={null} orientation="white" />);
    expect(screen.getByRole("img", { name: "No evaluation yet" })).toBeInTheDocument();
    expect(container.querySelector<HTMLElement>(".eval-bar__white")!.style.height).toBe("50%");
  });

  it("fills the bar completely for a forced mate and shows it", () => {
    const { container } = render(<EvalBar value={{ kind: "mate", value: -3 }} orientation="white" />);
    expect(screen.getByRole("img", { name: "Evaluation -M3" })).toBeInTheDocument();
    expect(container.querySelector<HTMLElement>(".eval-bar__white")!.style.height).toBe("0%");
  });

  it("flips with the board", () => {
    const { container, rerender } = render(<EvalBar value={{ kind: "cp", value: 100 }} orientation="white" />);
    expect(container.querySelector<HTMLElement>(".eval-bar")!.style.flexDirection).toBe("column");
    rerender(<EvalBar value={{ kind: "cp", value: 100 }} orientation="black" />);
    expect(container.querySelector<HTMLElement>(".eval-bar")!.style.flexDirection).toBe("column-reverse");
  });
});
