//! Background review jobs: start one, watch its events, cancel it. UI-agnostic, so the desktop
//! shell only has to forward `JobEvent`s to the frontend.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use crate::engine::{Analyzer, EngineError};
use crate::eval::Eval;
use crate::game::{Game, GameError, parse_pgn};
use crate::openings::OpeningBook;
use crate::review::{MoveReview, ReviewError, ReviewEvent, ReviewOptions, review_game_streaming};
use crate::settings::{Settings, SettingsError};
use crate::store::GameStore;

pub type JobId = u32;

/// Progress of a running job, in the order a UI wants to draw it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobEvent {
    /// Position `index` (0 = the start) has been analysed.
    Analysed {
        job: JobId,
        index: usize,
        total: usize,
        eval: Eval,
    },
    /// A move has been classified.
    Move {
        job: JobId,
        mv: MoveReview,
    },
    /// The review finished and was saved as `game_id`.
    Complete {
        job: JobId,
        #[ts(type = "number")]
        game_id: i64,
    },
    Failed {
        job: JobId,
        message: String,
    },
    Cancelled {
        job: JobId,
    },
}

/// Where the game to review comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewSource {
    /// Game number `game_index` (0-based) of a PGN text.
    Pgn { text: String, game_index: usize },
    /// A game recorded move by move, in UCI notation.
    Moves {
        start_fen: Option<String>,
        uci_moves: Vec<String>,
        headers: BTreeMap<String, String>,
    },
}

#[derive(Debug, Error, PartialEq)]
pub enum JobError {
    #[error(transparent)]
    Game(#[from] GameError),
    #[error("game {index} does not exist: the PGN contains {count} game(s)")]
    NoSuchGame { index: usize, count: usize },
    #[error(transparent)]
    Settings(#[from] SettingsError),
}

/// Receives job events, from whichever thread the job runs on.
/// What `ReviewJobs::start` hands back: the job to watch and the game it is reviewing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StartedJob {
    pub job: JobId,
    pub game: Game,
}

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: JobEvent);
}

/// Builds the analyzer for one job from the current settings.
pub type EngineFactory =
    dyn Fn(&Settings) -> Result<Box<dyn Analyzer + Send>, EngineError> + Send + Sync;

struct Running {
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

pub struct ReviewJobs {
    next_id: AtomicU32,
    running: Arc<Mutex<HashMap<JobId, Running>>>,
    factory: Arc<EngineFactory>,
    store: Arc<Mutex<GameStore>>,
    sink: Arc<dyn EventSink>,
    book: &'static OpeningBook,
}

impl ReviewJobs {
    pub fn new(
        factory: Arc<EngineFactory>,
        store: Arc<Mutex<GameStore>>,
        sink: Arc<dyn EventSink>,
        book: &'static OpeningBook,
    ) -> ReviewJobs {
        ReviewJobs {
            next_id: AtomicU32::new(1),
            running: Arc::new(Mutex::new(HashMap::new())),
            factory,
            store,
            sink,
            book,
        }
    }

    /// Validates the request and the game, then reviews it on a background thread.
    /// Problems with the request come back here; problems while reviewing arrive as events.
    pub fn start(&self, source: ReviewSource, settings: Settings) -> Result<StartedJob, JobError> {
        settings.validate()?;
        let game = build_game(source)?;

        let job = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let context = JobContext {
            job,
            game: game.clone(),
            settings,
            cancel: Arc::clone(&cancel),
            factory: Arc::clone(&self.factory),
            store: Arc::clone(&self.store),
            sink: Arc::clone(&self.sink),
            book: self.book,
        };
        let running = Arc::clone(&self.running);
        // Hold the lock across the spawn so the thread cannot finish and deregister
        // before it has been registered.
        let mut guard = self.running.lock().expect("jobs lock");
        let thread = std::thread::spawn(move || {
            run_job(context);
            running.lock().expect("jobs lock").remove(&job);
        });
        guard.insert(
            job,
            Running {
                cancel,
                thread: Some(thread),
            },
        );
        Ok(StartedJob { job, game })
    }

    /// Asks a running job to stop. Returns false if there is no such running job.
    pub fn cancel(&self, job: JobId) -> bool {
        match self.running.lock().expect("jobs lock").get(&job) {
            Some(running) => {
                running.cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    pub fn is_running(&self, job: JobId) -> bool {
        self.running.lock().expect("jobs lock").contains_key(&job)
    }

    /// Blocks until the job has finished. For tests and orderly shutdown.
    pub fn wait(&self, job: JobId) {
        let thread = self
            .running
            .lock()
            .expect("jobs lock")
            .get_mut(&job)
            .and_then(|running| running.thread.take());
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

fn build_game(source: ReviewSource) -> Result<Game, JobError> {
    match source {
        ReviewSource::Pgn { text, game_index } => {
            let mut games = parse_pgn(&text)?;
            if game_index >= games.len() {
                return Err(JobError::NoSuchGame {
                    index: game_index,
                    count: games.len(),
                });
            }
            Ok(games.swap_remove(game_index))
        }
        ReviewSource::Moves {
            start_fen,
            uci_moves,
            headers,
        } => Ok(Game::from_uci_moves(
            start_fen.as_deref(),
            &uci_moves,
            headers,
        )?),
    }
}

struct JobContext {
    job: JobId,
    game: Game,
    settings: Settings,
    cancel: Arc<AtomicBool>,
    factory: Arc<EngineFactory>,
    store: Arc<Mutex<GameStore>>,
    sink: Arc<dyn EventSink>,
    book: &'static OpeningBook,
}

fn run_job(context: JobContext) {
    let JobContext {
        job,
        game,
        settings,
        cancel,
        factory,
        store,
        sink,
        book,
    } = context;

    let mut analyzer = match factory(&settings) {
        Ok(analyzer) => analyzer,
        Err(e) => {
            sink.emit(JobEvent::Failed {
                job,
                message: e.to_string(),
            });
            return;
        }
    };
    let options = ReviewOptions {
        limits: settings.limits(),
        ..ReviewOptions::default()
    };
    let result =
        review_game_streaming(&game, analyzer.as_mut(), &options, book, &cancel, |event| {
            sink.emit(match event {
                ReviewEvent::Analysed { index, total, eval } => JobEvent::Analysed {
                    job,
                    index,
                    total,
                    eval,
                },
                ReviewEvent::Move(mv) => JobEvent::Move { job, mv },
            });
        });

    sink.emit(match result {
        Ok(review) => match store.lock().expect("store lock").save(&game, &review) {
            Ok(game_id) => JobEvent::Complete { job, game_id },
            Err(e) => JobEvent::Failed {
                job,
                message: format!("the review finished but could not be saved: {e}"),
            },
        },
        Err(ReviewError::Cancelled) => JobEvent::Cancelled { job },
        Err(e) => JobEvent::Failed {
            job,
            message: e.to_string(),
        },
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AnalysisLine, PositionAnalysis, ScriptedAnalyzer};
    use std::sync::mpsc::{Receiver, channel};

    #[derive(Default)]
    struct Collect(Mutex<Vec<JobEvent>>);

    impl EventSink for Collect {
        fn emit(&self, event: JobEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    impl Collect {
        fn events(&self) -> Vec<JobEvent> {
            self.0.lock().unwrap().clone()
        }
    }

    fn line(eval: Eval, first_move: &str) -> AnalysisLine {
        AnalysisLine {
            rank: 1,
            eval,
            depth: 20,
            pv: vec![first_move.to_string()],
        }
    }

    /// Analyses for "1. f3 e5 2. g4 Qh4#": four engine calls (the mated position is not sent).
    fn fools_mate_script() -> Vec<PositionAnalysis> {
        [
            (Eval::Cp(20), "e2e4"),
            (Eval::Cp(-60), "e7e5"),
            (Eval::Cp(-50), "d2d4"),
            (Eval::Mate(-1), "d8h4"),
        ]
        .into_iter()
        .map(|(eval, mv)| PositionAnalysis {
            lines: vec![line(eval, mv)],
        })
        .collect()
    }

    const FOOLS_MATE: &str =
        "[White \"A\"]\n[Black \"B\"]\n[Result \"0-1\"]\n\n1. f3 e5 2. g4 Qh4# 0-1\n";

    fn empty_book() -> &'static OpeningBook {
        Box::leak(Box::new(OpeningBook::empty()))
    }

    fn scripted_factory(script: Vec<PositionAnalysis>) -> Arc<EngineFactory> {
        Arc::new(move |_settings: &Settings| {
            Ok(Box::new(ScriptedAnalyzer::new(script.clone())) as Box<dyn Analyzer + Send>)
        })
    }

    fn jobs_with(factory: Arc<EngineFactory>) -> (ReviewJobs, Arc<Collect>, Arc<Mutex<GameStore>>) {
        let sink = Arc::new(Collect::default());
        let store = Arc::new(Mutex::new(GameStore::in_memory().unwrap()));
        let jobs = ReviewJobs::new(factory, Arc::clone(&store), sink.clone(), empty_book());
        (jobs, sink, store)
    }

    fn start_job(jobs: &ReviewJobs, source: ReviewSource, settings: Settings) -> JobId {
        jobs.start(source, settings).unwrap().job
    }

    fn pgn_source(text: &str) -> ReviewSource {
        ReviewSource::Pgn {
            text: text.to_string(),
            game_index: 0,
        }
    }

    #[test]
    fn a_pgn_review_streams_events_then_saves_the_game() {
        let (jobs, sink, store) = jobs_with(scripted_factory(fools_mate_script()));
        let job = start_job(&jobs, pgn_source(FOOLS_MATE), Settings::default());
        jobs.wait(job);

        let events = sink.events();
        let shape: Vec<String> = events
            .iter()
            .map(|e| match e {
                JobEvent::Analysed { index, .. } => format!("a{index}"),
                JobEvent::Move { mv, .. } => format!("m{}", mv.ply),
                JobEvent::Complete { .. } => "done".to_string(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            shape,
            ["a0", "a1", "m1", "a2", "m2", "a3", "m3", "a4", "m4", "done"]
        );
        assert!(
            events.iter().all(|e| match e {
                JobEvent::Analysed { job: j, .. }
                | JobEvent::Move { job: j, .. }
                | JobEvent::Complete { job: j, .. } => *j == job,
                _ => false,
            }),
            "every event carries the job id"
        );

        let saved = store.lock().unwrap().list().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].white, "A");
        match events.last().unwrap() {
            JobEvent::Complete { game_id, .. } => assert_eq!(*game_id, saved[0].id),
            other => panic!("expected Complete, got {other:?}"),
        }
        assert!(!jobs.is_running(job));
    }

    #[test]
    fn starting_a_job_returns_the_parsed_game_so_the_ui_can_draw_the_board() {
        let (jobs, _sink, _store) = jobs_with(scripted_factory(fools_mate_script()));
        let started = jobs
            .start(pgn_source(FOOLS_MATE), Settings::default())
            .unwrap();
        jobs.wait(started.job);
        assert_eq!(started.game.moves.len(), 4);
        assert_eq!(started.game.positions.len(), 5);
        assert_eq!(started.game.headers["White"], "A");
    }

    #[test]
    fn a_recorded_game_can_be_reviewed_from_uci_moves() {
        let (jobs, sink, store) = jobs_with(scripted_factory(fools_mate_script()));
        let source = ReviewSource::Moves {
            start_fen: None,
            uci_moves: ["f2f3", "e7e5", "g2g4", "d8h4"].map(String::from).to_vec(),
            headers: BTreeMap::from([("White".to_string(), "Me".to_string())]),
        };
        let job = start_job(&jobs, source, Settings::default());
        jobs.wait(job);
        assert!(matches!(
            sink.events().last(),
            Some(JobEvent::Complete { .. })
        ));
        assert_eq!(store.lock().unwrap().list().unwrap()[0].white, "Me");
    }

    #[test]
    fn the_requested_game_of_a_multi_game_pgn_is_reviewed() {
        let (jobs, _sink, store) = jobs_with(scripted_factory(fools_mate_script()));
        let text = format!("[White \"First\"]\n\n1. e4 *\n\n{FOOLS_MATE}");
        let job = start_job(
            &jobs,
            ReviewSource::Pgn {
                text,
                game_index: 1,
            },
            Settings::default(),
        );
        jobs.wait(job);
        assert_eq!(store.lock().unwrap().list().unwrap()[0].white, "A");
    }

    #[test]
    fn bad_requests_fail_immediately_and_start_nothing() {
        let (jobs, sink, _store) = jobs_with(scripted_factory(vec![]));
        assert!(matches!(
            jobs.start(pgn_source("not a game"), Settings::default()),
            Err(JobError::Game(GameError::Empty))
        ));
        assert_eq!(
            jobs.start(
                ReviewSource::Pgn {
                    text: FOOLS_MATE.to_string(),
                    game_index: 3
                },
                Settings::default()
            ),
            Err(JobError::NoSuchGame { index: 3, count: 1 })
        );
        let bad_settings = Settings {
            depth: 0,
            ..Settings::default()
        };
        assert!(matches!(
            jobs.start(pgn_source(FOOLS_MATE), bad_settings),
            Err(JobError::Settings(_))
        ));
        assert!(
            jobs.start(
                ReviewSource::Moves {
                    start_fen: None,
                    uci_moves: vec!["e2e5".into()],
                    headers: BTreeMap::new()
                },
                Settings::default()
            )
            .is_err()
        );
        assert!(sink.events().is_empty());
    }

    #[test]
    fn a_missing_engine_is_reported_as_a_failed_job() {
        let factory: Arc<EngineFactory> = Arc::new(|_: &Settings| Err(EngineError::NotFound));
        let (jobs, sink, store) = jobs_with(factory);
        let job = start_job(&jobs, pgn_source(FOOLS_MATE), Settings::default());
        jobs.wait(job);
        match sink.events().as_slice() {
            [JobEvent::Failed { message, .. }] => {
                assert!(message.contains("Stockfish was not found"), "{message}");
            }
            other => panic!("unexpected events {other:?}"),
        }
        assert!(store.lock().unwrap().list().unwrap().is_empty());
    }

    #[test]
    fn an_engine_failure_midway_is_reported_and_nothing_is_saved() {
        let mut script = fools_mate_script();
        script.truncate(2); // the third position gets no answer
        let (jobs, sink, store) = jobs_with(scripted_factory(script));
        let job = start_job(&jobs, pgn_source(FOOLS_MATE), Settings::default());
        jobs.wait(job);
        assert!(matches!(
            sink.events().last(),
            Some(JobEvent::Failed { .. })
        ));
        assert!(store.lock().unwrap().list().unwrap().is_empty());
    }

    /// An analyzer that blocks on its first call until the test releases it.
    struct Gated {
        gate: Receiver<()>,
        inner: ScriptedAnalyzer,
        opened: bool,
    }

    impl Analyzer for Gated {
        fn analyze(
            &mut self,
            fen: &str,
            limits: &crate::engine::Limits,
        ) -> Result<PositionAnalysis, EngineError> {
            if !self.opened {
                let _ = self.gate.recv();
                self.opened = true;
            }
            self.inner.analyze(fen, limits)
        }

        fn engine_id(&self) -> String {
            "gated".into()
        }
    }

    #[test]
    fn cancelling_stops_the_job_without_saving() {
        let (release, gate) = channel();
        let gate = Mutex::new(Some(gate));
        let factory: Arc<EngineFactory> = Arc::new(move |_: &Settings| {
            Ok(Box::new(Gated {
                gate: gate.lock().unwrap().take().expect("one job"),
                inner: ScriptedAnalyzer::new(fools_mate_script()),
                opened: false,
            }) as Box<dyn Analyzer + Send>)
        });
        let (jobs, sink, store) = jobs_with(factory);
        let job = start_job(&jobs, pgn_source(FOOLS_MATE), Settings::default());

        assert!(jobs.is_running(job));
        assert!(jobs.cancel(job));
        release.send(()).unwrap();
        jobs.wait(job);

        assert!(matches!(
            sink.events().last(),
            Some(JobEvent::Cancelled { .. })
        ));
        assert!(store.lock().unwrap().list().unwrap().is_empty());
        assert!(!jobs.cancel(job), "a finished job cannot be cancelled");
    }

    #[test]
    fn jobs_get_distinct_ids() {
        let (jobs, _sink, _store) = jobs_with(scripted_factory(fools_mate_script()));
        let first = start_job(&jobs, pgn_source(FOOLS_MATE), Settings::default());
        jobs.wait(first);
        let second = start_job(&jobs, pgn_source(FOOLS_MATE), Settings::default());
        jobs.wait(second);
        assert_ne!(first, second);
    }

    #[test]
    fn events_serialize_for_the_frontend() {
        let json = serde_json::to_string(&JobEvent::Complete { job: 7, game_id: 3 }).unwrap();
        assert_eq!(json, r#"{"kind":"complete","job":7,"game_id":3}"#);
        let json = serde_json::to_string(&JobEvent::Cancelled { job: 1 }).unwrap();
        assert_eq!(json, r#"{"kind":"cancelled","job":1}"#);
    }
}
