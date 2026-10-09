import { useMemo, useState } from "react";
import { BoardPanel } from "../components/BoardPanel";
import { EvalBar } from "../components/EvalBar";
import { EvalGraph } from "../components/EvalGraph";
import { GameDetails } from "../components/GameDetails";
import { LiveLines } from "../components/LiveLines";
import { MoveList } from "../components/MoveList";
import { NavControls } from "../components/NavControls";
import { PromotionChooser } from "../components/PromotionChooser";
import { SummaryPanel } from "../components/SummaryPanel";
import type { ReviewSource } from "../generated/ReviewSource";
import { useBoardEntry } from "../hooks/useBoardEntry";
import { commentaryFor } from "../lib/commentary";
import { type LiveState, liveBoard, liveView } from "../lib/live";
import {
  type RecordDraft,
  emptyDraft,
  gameOver,
  recordedGame,
  statusText,
  takeBack,
  toReviewSource,
  trySan,
} from "../lib/record";
import { moveRows } from "../lib/rows";

export interface LiveScreenProps {
  /** The game being followed. It lives in the parent so it survives leaving this screen. */
  draft: RecordDraft;
  onDraftChange: (draft: RecordDraft) => void;
  /** What the engine has said so far about this game. */
  live: LiveState;
  /** Asks the backend to start the analysis again (after it stopped). */
  onRestart: () => void;
  onReview: (source: ReviewSource) => void;
  /** Leaves the screen. The game is kept. */
  onBack: () => void;
}

/**
 * Follow a game that is being played elsewhere: enter its moves, for both sides, and watch the
 * engine's verdict build up. Moves are entered on the board or typed in algebraic notation.
 */
export function LiveScreen({ draft, onDraftChange, live, onRestart, onReview, onBack }: LiveScreenProps) {
  const { recording, white, black, result } = draft;
  const update = (patch: Partial<RecordDraft>) => onDraftChange({ ...draft, ...patch });
  const entry = useBoardEntry(recording, (next) => update({ recording: next }));

  const [orientation, setOrientation] = useState<"white" | "black">("white");
  const [showNext, setShowNext] = useState(true);
  const [confirmingNew, setConfirmingNew] = useState(false);
  const [text, setText] = useState("");
  const [refusal, setRefusal] = useState<string | null>(null);
  // null follows the latest move; a number means the user is looking back at that move.
  const [looking, setLooking] = useState<number | null>(null);

  const moveCount = recording.uciMoves.length;
  const ply = Math.min(looking ?? moveCount, moveCount);
  const atEnd = ply === moveCount;
  const ended = gameOver(recording);
  const canEnter = atEnd && !ended.over;

  const headers = { white, black, result };
  const game = useMemo(() => recordedGame(recording, headers), [recording, white, black, result]);
  const view = useMemo(() => liveView(live, game), [live, game]);
  const board = liveBoard(view, ply, showNext);
  const rows = useMemo(() => moveRows(view.data, view.provisional), [view]);

  const select = (next: number) => setLooking(next >= moveCount ? null : Math.max(next, 0));

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const played = trySan(recording, text);
    if ("error" in played) {
      setRefusal(played.error);
      return;
    }
    update({ recording: played.state });
    setText("");
    setRefusal(null);
  };

  const startOver = () => {
    onDraftChange(emptyDraft);
    entry.cancelPromotion();
    setConfirmingNew(false);
    setLooking(null);
    setText("");
    setRefusal(null);
  };

  return (
    <div className="review live">
      <header className="review__header">
        <button type="button" onClick={onBack}>
          ← Back
        </button>
        <h1>Live game</h1>
      </header>

      {live.error && (
        <div className="banner banner--error" role="alert">
          <span>The analysis stopped: {live.error}</span>
          <button type="button" onClick={onRestart}>
            Restart analysis
          </button>
        </div>
      )}

      <div className="review__body">
        <div className="review__board">
          <div className="review__board-row">
            <EvalBar value={view.data.evals[ply] ?? null} orientation={orientation} />
            <BoardPanel
              fen={board.fen}
              orientation={orientation}
              lastMove={board.lastMove}
              bestArrow={board.nextArrow}
              missArrow={board.missArrow}
              badge={board.badge}
              onMove={canEnter ? entry.handleMove : undefined}
            />
          </div>
          <p className="review__commentary" role="status">
            {statusText(recording)}
          </p>
          {ply > 0 && (
            <p className="review__commentary live__commentary" aria-live="polite">
              {commentaryFor(view.data.moves[ply - 1], ply)}
              {view.data.moves[ply - 1] && view.provisional[ply - 1] && (
                <span className="live__provisional"> This may change as the engine searches deeper.</span>
              )}
            </p>
          )}
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

          <form className="live__entry" onSubmit={submit}>
            <label htmlFor="live-san">Move</label>
            <input
              id="live-san"
              type="text"
              value={text}
              placeholder="e4, Nf3, O-O…"
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              disabled={!canEnter}
              onChange={(e) => {
                setText(e.target.value);
                setRefusal(null);
              }}
            />
            <button type="submit" disabled={!canEnter || text.trim() === ""}>
              Play
            </button>
          </form>
          {refusal && (
            <p className="live__refusal" role="alert">
              {refusal}
            </p>
          )}
          {!atEnd && <p className="live__hint">Go to the latest move to enter more.</p>}

          <NavControls
            ply={ply}
            last={moveCount}
            onSelect={select}
            onFlip={() => setOrientation((o) => (o === "white" ? "black" : "white"))}
            showBest={showNext}
            onToggleBest={() => setShowNext((s) => !s)}
            toggleLabel="Current best move"
          />
          <div className="nav-controls">
            <button
              type="button"
              onClick={() => {
                update({ recording: takeBack(recording) });
                setLooking(null);
              }}
              disabled={moveCount === 0}
            >
              Take back
            </button>
            <button
              type="button"
              onClick={() => (moveCount === 0 ? startOver() : setConfirmingNew(true))}
            >
              New game
            </button>
          </div>
        </div>

        <div className="review__side">
          <GameDetails
            idPrefix="live"
            white={white}
            black={black}
            result={result}
            ended={ended}
            onChange={update}
          />
          <SummaryPanel data={view.data} hideOpening />
          <LiveLines position={view.positions[ply] ?? null} fen={board.fen} />
          <EvalGraph
            evals={view.data.evals}
            moves={view.data.moves}
            selectedPly={ply}
            onSelect={select}
          />
          <MoveList rows={rows} selectedPly={ply} onSelect={select} />
          <button
            type="button"
            className="primary"
            disabled={moveCount === 0}
            onClick={() => onReview(toReviewSource(recording, headers))}
          >
            Review this game
          </button>
        </div>
      </div>
    </div>
  );
}
