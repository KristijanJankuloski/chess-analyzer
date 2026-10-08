import { useCallback, useState } from "react";
import type { Api } from "./api/types";
import { type JobState, useReviewJob } from "./hooks/useReviewJob";
import { type RecordDraft, emptyDraft } from "./lib/record";
import type { ReviewData } from "./lib/reviewData";
import { HomeScreen } from "./screens/HomeScreen";
import { RecordScreen } from "./screens/RecordScreen";
import { ReviewScreen } from "./screens/ReviewScreen";
import { SettingsScreen } from "./screens/SettingsScreen";

interface Reviewing {
  data: ReviewData;
  status: "running" | "complete" | "failed" | "cancelled";
  message?: string;
}

/** What the review screen should show, or null when there is nothing to review (yet). */
function reviewing(state: JobState): Reviewing | null {
  switch (state.status) {
    case "idle":
      return null;
    case "failed":
      // A request that was rejected before any analysis has nothing to show; stay on the home screen.
      return state.data ? { data: state.data, status: "failed", message: state.message } : null;
    default:
      return { data: state.data, status: state.status };
  }
}

export function App({ api }: { api: Api }) {
  const job = useReviewJob(api);
  const [view, setView] = useState<"home" | "settings" | "record">("home");
  // The game being entered by hand lives here, so it survives looking at another screen.
  const [draft, setDraft] = useState<RecordDraft>(emptyDraft);
  const { state } = job;
  const current = reviewing(state);
  // A new key each time a review is started or opened, so the screen begins at move 0 again.
  const [reviewKey, setReviewKey] = useState(0);
  const { start: startJob, open: openJob } = job;
  const start = useCallback(
    (source: Parameters<typeof startJob>[0]) => {
      setReviewKey((k) => k + 1);
      return startJob(source);
    },
    [startJob],
  );
  const open = useCallback(
    (gameId: number) => {
      setReviewKey((k) => k + 1);
      return openJob(gameId);
    },
    [openJob],
  );

  const goHome = () => {
    if (state.status === "running") job.cancel();
    job.close();
    setView("home");
  };

  let body: React.ReactNode;
  if (view === "settings") {
    body = <SettingsScreen api={api} onDone={() => setView("home")} />;
  } else if (view === "record" && !current) {
    body = (
      <RecordScreen
        draft={draft}
        onDraftChange={setDraft}
        onCancel={() => setView("home")}
        onReview={(source) => {
          setDraft(emptyDraft);
          setView("home");
          void start(source);
        }}
      />
    );
  } else if (current) {
    body = (
      <ReviewScreen
        key={reviewKey}
        data={current.data}
        status={current.status}
        message={current.message}
        onCancel={job.cancel}
        onBack={goHome}
      />
    );
  } else {
    body = (
      <HomeScreen
        api={api}
        onStart={start}
        onOpen={open}
        notice={state.status === "failed" ? state.message : null}
      />
    );
  }

  return (
    <div className="app">
      <nav className="app__nav">
        <strong className="app__title">Chess Analyzer</strong>
        <button type="button" onClick={goHome} aria-current={view === "home" ? "page" : undefined}>
          Games
        </button>
        <button
          type="button"
          onClick={() => {
            if (state.status === "running") job.cancel();
            job.close();
            setView("record");
          }}
          aria-current={view === "record" ? "page" : undefined}
        >
          Record
        </button>
        <button
          type="button"
          onClick={() => setView("settings")}
          aria-current={view === "settings" ? "page" : undefined}
        >
          Settings
        </button>
      </nav>
      <main className="app__main">{body}</main>
    </div>
  );
}
