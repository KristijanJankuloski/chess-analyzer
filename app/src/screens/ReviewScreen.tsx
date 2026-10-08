import { useCallback, useMemo, useState } from "react";
import { BoardPanel } from "../components/BoardPanel";
import { EvalBar } from "../components/EvalBar";
import { EvalGraph } from "../components/EvalGraph";
import { MoveList } from "../components/MoveList";
import { NavControls } from "../components/NavControls";
import { SummaryPanel } from "../components/SummaryPanel";
import { boardView } from "../lib/board";
import { describeMove } from "../lib/commentary";
import type { ReviewData } from "../lib/reviewData";
import { moveRows } from "../lib/rows";

export interface ReviewScreenProps {
  data: ReviewData;
  status: "running" | "complete" | "failed" | "cancelled";
  /** Why the review failed, when it did. */
  message?: string;
  onCancel: () => void;
  onBack: () => void;
}

function Banner({
  data,
  status,
  message,
  onCancel,
}: Pick<ReviewScreenProps, "data" | "status" | "message" | "onCancel">) {
  if (status === "running") {
    return (
      <div className="banner" role="status">
        <span>
          Analysing position {Math.min(data.analysed + 1, data.evals.length)} of {data.evals.length}
        </span>
        <progress max={data.evals.length} value={data.analysed} aria-label="Analysis progress" />
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    );
  }
  if (status === "failed") {
    return (
      <div className="banner banner--error" role="alert">
        The review stopped: {message ?? "unknown error"}
      </div>
    );
  }
  if (status === "cancelled") {
    return (
      <div className="banner" role="status">
        Review cancelled. Showing what was analysed.
      </div>
    );
  }
  return null;
}

export function ReviewScreen({ data, status, message, onCancel, onBack }: ReviewScreenProps) {
  const last = data.game.moves.length;
  const [ply, setPly] = useState(0);
  const [orientation, setOrientation] = useState<"white" | "black">("white");
  const [showBest, setShowBest] = useState(true);

  const select = useCallback((next: number) => setPly(Math.min(Math.max(next, 0), last)), [last]);
  const view = boardView(data, ply, showBest);
  const rows = useMemo(() => moveRows(data), [data]);
  const header = (key: string) => data.game.headers[key] ?? "?";

  return (
    <div className="review">
      <header className="review__header">
        <button type="button" onClick={onBack}>
          ← Back
        </button>
        <h1>
          {header("White")} vs {header("Black")}
          <span className="review__result"> {header("Result")}</span>
        </h1>
      </header>

      <Banner data={data} status={status} message={message} onCancel={onCancel} />

      <div className="review__body">
        <div className="review__board">
          <div className="review__board-row">
            <EvalBar value={data.evals[ply] ?? null} orientation={orientation} />
            <BoardPanel
              fen={view.fen}
              orientation={orientation}
              lastMove={view.lastMove}
              bestArrow={view.bestArrow}
              badge={view.badge}
            />
          </div>
          <p className="review__commentary" aria-live="polite">
            {describeMove(ply === 0 ? null : data.moves[ply - 1], ply)}
          </p>
          <NavControls
            ply={ply}
            last={last}
            onSelect={select}
            onFlip={() => setOrientation((o) => (o === "white" ? "black" : "white"))}
            showBest={showBest}
            onToggleBest={() => setShowBest((s) => !s)}
          />
        </div>

        <div className="review__side">
          <SummaryPanel data={data} />
          <EvalGraph evals={data.evals} moves={data.moves} selectedPly={ply} onSelect={select} />
          <MoveList rows={rows} selectedPly={ply} onSelect={select} />
        </div>
      </div>
    </div>
  );
}
