import { useMemo, useState } from "react";
import { BoardPanel } from "../components/BoardPanel";
import { MoveList } from "../components/MoveList";
import type { ReviewSource } from "../generated/ReviewSource";
import { uciSquares } from "../lib/board";
import {
  type RecordResult,
  type RecordState,
  currentFen,
  emptyRecording,
  gameOver,
  isPromotion,
  recordedGame,
  takeBack,
  toReviewSource,
  tryMove,
  turn,
} from "../lib/record";
import { startLive } from "../lib/reviewData";
import { moveRows } from "../lib/rows";

export interface RecordScreenProps {
  onReview: (source: ReviewSource) => void;
  onCancel: () => void;
}

const PROMOTIONS = [
  { label: "Queen", piece: "q" },
  { label: "Rook", piece: "r" },
  { label: "Bishop", piece: "b" },
  { label: "Knight", piece: "n" },
];

const RESULTS: { value: RecordResult; label: string }[] = [
  { value: "*", label: "Unfinished" },
  { value: "1-0", label: "1-0 (White won)" },
  { value: "0-1", label: "0-1 (Black won)" },
  { value: "1/2-1/2", label: "½-½ (draw)" },
];

function statusText(state: RecordState): string {
  const ended = gameOver(state);
  if (!ended.over) return `${turn(state) === "white" ? "White" : "Black"} to move`;
  return ended.reason === "checkmate" ? `Checkmate: ${ended.result}` : `Draw by ${ended.reason}`;
}

/** Enter a game move by move on the board, then send it off to be reviewed. */
export function RecordScreen({ onReview, onCancel }: RecordScreenProps) {
  const [recording, setRecording] = useState<RecordState>(emptyRecording);
  const [orientation, setOrientation] = useState<"white" | "black">("white");
  const [white, setWhite] = useState("");
  const [black, setBlack] = useState("");
  const [result, setResult] = useState<RecordResult>("*");
  const [pending, setPending] = useState<{ from: string; to: string } | null>(null);

  const headers = { white, black, result };
  const ended = gameOver(recording);
  const game = useMemo(() => recordedGame(recording, { white, black, result }), [recording, white, black, result]);
  const rows = useMemo(() => moveRows(startLive(game)), [game]);
  const lastUci = recording.uciMoves.at(-1);

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

  return (
    <div className="review">
      <header className="review__header">
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
        <h1>Record a game</h1>
      </header>

      <div className="review__body">
        <div className="review__board">
          <BoardPanel
            fen={currentFen(recording)}
            orientation={orientation}
            lastMove={lastUci ? uciSquares(lastUci) : null}
            onMove={ended.over ? undefined : handleMove}
          />
          <p className="review__commentary" role="status">
            {statusText(recording)}
          </p>
          {pending && (
            <div role="group" aria-label="Promote to" className="record__promotion">
              {PROMOTIONS.map(({ label, piece }) => (
                <button key={piece} type="button" onClick={() => promote(piece)}>
                  {label}
                </button>
              ))}
              <button type="button" onClick={() => setPending(null)}>
                Cancel promotion
              </button>
            </div>
          )}
          <div className="nav-controls">
            <button
              type="button"
              onClick={() => setRecording(takeBack(recording))}
              disabled={recording.uciMoves.length === 0}
            >
              Take back
            </button>
            <button
              type="button"
              onClick={() => {
                setRecording(emptyRecording);
                setPending(null);
              }}
            >
              New game
            </button>
            <span className="nav-controls__spacer" />
            <button
              type="button"
              onClick={() => setOrientation((o) => (o === "white" ? "black" : "white"))}
              aria-label="Flip board"
            >
              ⇅
            </button>
          </div>
        </div>

        <div className="review__side">
          <div className="summary record__details">
            <label htmlFor="record-white">White</label>
            <input id="record-white" type="text" value={white} placeholder="White" onChange={(e) => setWhite(e.target.value)} />
            <label htmlFor="record-black">Black</label>
            <input id="record-black" type="text" value={black} placeholder="Black" onChange={(e) => setBlack(e.target.value)} />
            <label htmlFor="record-result">Result</label>
            <select
              id="record-result"
              value={ended.over ? ended.result : result}
              disabled={ended.over}
              onChange={(e) => setResult(e.target.value as RecordResult)}
            >
              {(ended.over ? RESULTS.filter((r) => r.value === ended.result) : RESULTS).map((r) => (
                <option key={r.value} value={r.value}>
                  {r.label}
                </option>
              ))}
            </select>
          </div>

          <MoveList rows={rows} selectedPly={recording.uciMoves.length} onSelect={() => {}} />

          <button
            type="button"
            className="primary"
            disabled={recording.uciMoves.length === 0}
            onClick={() => onReview(toReviewSource(recording, headers))}
          >
            Review this game
          </button>
        </div>
      </div>
    </div>
  );
}
