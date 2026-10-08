import { useState } from "react";
import { type RecordState, isPromotion, tryMove } from "../lib/record";

export interface PendingPromotion {
  from: string;
  to: string;
}

/**
 * Turns moves made on the board into moves of a recording. A promotion is held back until the
 * player has chosen a piece (`pending` says there is one waiting).
 */
export function useBoardEntry(recording: RecordState, setRecording: (next: RecordState) => void) {
  const [pending, setPending] = useState<PendingPromotion | null>(null);

  /** For the board's `onMove`: true if the move was played, false if the piece should snap back. */
  const handleMove = (from: string, to: string): boolean => {
    if (isPromotion(recording, from, to)) {
      setPending({ from, to });
      return false; // the piece snaps back until a promotion piece is chosen
    }
    const next = tryMove(recording, from, to);
    if (!next) return false;
    setRecording(next);
    return true;
  };

  const promote = (piece: string) => {
    if (!pending) return;
    const next = tryMove(recording, pending.from, pending.to, piece);
    if (next) setRecording(next);
    setPending(null);
  };

  const cancelPromotion = () => setPending(null);

  return { pending, handleMove, promote, cancelPromotion };
}
