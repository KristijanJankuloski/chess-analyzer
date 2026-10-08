import type { ChessboardOptions } from "react-chessboard";

let lastOptions: ChessboardOptions | undefined;

/** The options the most recently rendered (stubbed) board was given. */
export function lastBoardOptions(): ChessboardOptions {
  if (!lastOptions) throw new Error("no board has been rendered");
  return lastOptions;
}

/** Stands in for react-chessboard in tests: records its options and renders its position. */
export function Chessboard({ options }: { options?: ChessboardOptions }) {
  lastOptions = options;
  return <div data-testid="chessboard" data-fen={String(options?.position)} />;
}
