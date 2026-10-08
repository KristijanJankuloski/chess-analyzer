import { useMemo, useState } from "react";
import { BoardPanel } from "../components/BoardPanel";
import { GameDetails } from "../components/GameDetails";
import { MoveList } from "../components/MoveList";
import { PromotionChooser } from "../components/PromotionChooser";
import type { ReviewSource } from "../generated/ReviewSource";
import { useBoardEntry } from "../hooks/useBoardEntry";
import { uciSquares } from "../lib/board";
import {
  type RecordDraft,
  currentFen,
  emptyDraft,
  gameOver,
  recordedGame,
  statusText,
  takeBack,
  toReviewSource,
} from "../lib/record";
import { startLive } from "../lib/reviewData";
import { moveRows } from "../lib/rows";

export interface RecordScreenProps {
  /** The game being entered. It lives in the parent so it survives leaving this screen. */
  draft: RecordDraft;
  onDraftChange: (draft: RecordDraft) => void;
  onReview: (source: ReviewSource) => void;
  /** Leaves the screen. The draft is kept. */
  onCancel: () => void;
}

/** Enter a game move by move on the board, then send it off to be reviewed. */
export function RecordScreen({ draft, onDraftChange, onReview, onCancel }: RecordScreenProps) {
  const { recording, white, black, result } = draft;
  const update = (patch: Partial<RecordDraft>) => onDraftChange({ ...draft, ...patch });
  const entry = useBoardEntry(recording, (next) => update({ recording: next }));
  const [orientation, setOrientation] = useState<"white" | "black">("white");
  const [confirmingNew, setConfirmingNew] = useState(false);

  const headers = { white, black, result };
  const ended = gameOver(recording);
  const game = useMemo(() => recordedGame(recording, { white, black, result }), [recording, white, black, result]);
  const rows = useMemo(() => moveRows(startLive(game)), [game]);
  const lastUci = recording.uciMoves.at(-1);

  const startOver = () => {
    onDraftChange(emptyDraft);
    entry.cancelPromotion();
    setConfirmingNew(false);
  };

  return (
    <div className="review">
      <header className="review__header">
        <button type="button" onClick={onCancel}>
          ← Back
        </button>
        <h1>Record a game</h1>
      </header>

      <div className="review__body">
        <div className="review__board">
          <BoardPanel
            fen={currentFen(recording)}
            orientation={orientation}
            lastMove={lastUci ? uciSquares(lastUci) : null}
            onMove={ended.over ? undefined : entry.handleMove}
          />
          <p className="review__commentary" role="status">
            {statusText(recording)}
          </p>
          {entry.pending && (
            <PromotionChooser onChoose={entry.promote} onCancel={entry.cancelPromotion} />
          )}
          {confirmingNew && (
            <div role="group" aria-label="Discard this game?" className="record__promotion">
              <span>Discard this game?</span>
              <button type="button" onClick={startOver}>
                Discard game
              </button>
              <button type="button" onClick={() => setConfirmingNew(false)}>
                Keep playing
              </button>
            </div>
          )}
          <div className="nav-controls">
            <button
              type="button"
              onClick={() => update({ recording: takeBack(recording) })}
              disabled={recording.uciMoves.length === 0}
            >
              Take back
            </button>
            <button
              type="button"
              onClick={() => (recording.uciMoves.length === 0 ? startOver() : setConfirmingNew(true))}
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
          <GameDetails
            idPrefix="record"
            white={white}
            black={black}
            result={result}
            ended={ended}
            onChange={update}
          />

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
