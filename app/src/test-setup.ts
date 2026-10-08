import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

// The real board needs a browser layout engine; tests use a stub that records what it is asked to draw.
vi.mock("react-chessboard", async () => await import("./test-utils/boardStub"));

afterEach(() => cleanup());
