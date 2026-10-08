//! Following a game as it is played.
//!
//! A `LiveSession` owns one engine on a background thread. The caller sends the complete list of
//! moves whenever it changes; the session keeps what it already knows about the positions the
//! list shares with the previous one, searches the rest, and streams `LiveEvent`s: the engine's
//! lines for a position as they deepen, and a classification for every move as soon as the
//! analyses on both sides of it exist. The classification is `review::review_move`, the same
//! function a finished review uses, so a live badge can never mean something different.
//!
//! What gets searched, in order, after each change:
//!
//! 1. the newest position, to `QUICK_DEPTH`, so the evaluation bar moves at once;
//! 2. every earlier position that has no analysis yet (joining a game that is under way), newest
//!    first, to `BACKLOG_DEPTH`;
//! 3. the newest position again, with no depth limit, until the next change.
//!
//! A new change interrupts all of it and starts over from the analyses already stored.

use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use shakmaty::Chess;
use ts_rs::TS;

use crate::classify::{MoveClass, Thresholds};
use crate::engine::{
    AnalysisLine, EngineError, LiveEngine, PositionAnalysis, SearchLimit, SearchUpdate,
};
use crate::eval::Eval;
use crate::game::Game;
use crate::openings::OpeningBook;
use crate::review::{
    MoveReview, apply_uci, book_prefix, review_move, terminal_analysis, uci_to_san,
};

/// Depth of the first answer for the newest position.
pub const QUICK_DEPTH: u32 = 12;
/// Depth of the pass over positions that were never analysed.
pub const BACKLOG_DEPTH: u32 = 12;
/// A search reports every depth from 1; the first few are noise (the evaluation jumps around
/// and a move would flash through several classes), so they are not shown.
pub const MIN_SHOWN_DEPTH: u32 = 8;

/// How long the session waits for the engine before looking at its inbox again.
const POLL_WAIT: Duration = Duration::from_millis(50);

/// One line of the engine's analysis, with its moves also written in SAN.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LiveLine {
    /// 1 = the engine's best line.
    pub rank: u32,
    /// White's point of view.
    pub eval: Eval,
    pub depth: u32,
    /// The moves in UCI notation.
    pub pv: Vec<String>,
    /// The same moves in SAN, as far as they are legal from the position.
    pub pv_san: Vec<String>,
}

/// What a session reports. Every event carries the revision of the update it belongs to, so a
/// consumer can drop events that arrive after it has moved on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveEvent {
    /// The engine's lines for position `index` (0 = the start) at a new depth.
    Position {
        #[ts(type = "number")]
        revision: u64,
        index: usize,
        depth: u32,
        lines: Vec<LiveLine>,
    },
    /// Move `review.ply` was classified, or its classification changed. `provisional` means the
    /// analyses behind it may still deepen or were never deep.
    Move {
        #[ts(type = "number")]
        revision: u64,
        review: MoveReview,
        provisional: bool,
    },
    /// The engine failed or the move list was refused. The session keeps its state.
    Error {
        #[ts(type = "number")]
        revision: u64,
        message: String,
    },
}

/// Receives events from the session's thread.
pub trait LiveSink: Send + Sync + 'static {
    fn emit(&self, event: LiveEvent);
}

#[derive(Clone, Copy)]
pub struct LiveConfig {
    pub multipv: u32,
    pub thresholds: Thresholds,
    pub book: &'static OpeningBook,
}

enum Command {
    SetMoves { revision: u64, moves: Vec<String> },
    Pause,
    Shutdown,
}

/// A running live analysis. Dropping it stops the thread and the engine.
pub struct LiveSession {
    commands: Sender<Command>,
    latest_revision: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl LiveSession {
    pub fn start(
        engine: impl LiveEngine + Send + 'static,
        sink: Arc<dyn LiveSink>,
        config: LiveConfig,
    ) -> LiveSession {
        let (commands, inbox) = mpsc::channel();
        let latest_revision = Arc::new(AtomicU64::new(0));
        let latest = Arc::clone(&latest_revision);
        let thread = std::thread::spawn(move || {
            let report = Arc::clone(&sink);
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                Worker::new(engine, sink, config).run(&inbox);
            }));
            if outcome.is_err() {
                report.emit(LiveEvent::Error {
                    revision: latest.load(Ordering::Relaxed),
                    message: "the live analysis stopped unexpectedly (an internal error)"
                        .to_string(),
                });
            }
        });
        LiveSession {
            commands,
            latest_revision,
            thread: Some(thread),
        }
    }

    /// Tells the session the complete list of moves (UCI) played so far. `revision` must grow
    /// with every call; the events that follow carry it.
    pub fn set_moves(&self, revision: u64, moves: Vec<String>) {
        self.latest_revision.fetch_max(revision, Ordering::Relaxed);
        let _ = self.commands.send(Command::SetMoves { revision, moves });
    }

    /// Whether the worker thread is still there. It is not after an internal error (which is
    /// reported as a `LiveEvent::Error`); such a session can only be replaced.
    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    /// Stops searching. The analyses are kept; the next `set_moves` resumes.
    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Task {
    index: usize,
    limit: SearchLimit,
    /// Report every depth as it completes (the newest position) instead of only the last one.
    stream: bool,
}

struct Worker<E> {
    engine: E,
    sink: Arc<dyn LiveSink>,
    config: LiveConfig,
    revision: u64,
    game: Game,
    book_plies: usize,
    /// The newest position is a finished game, so it is never sent to the engine.
    live_terminal: bool,
    /// One slot per position of `game`.
    analyses: Vec<Option<PositionAnalysis>>,
    /// The last event sent for each move, to send only changes.
    sent: Vec<Option<(MoveReview, bool)>>,
    tasks: VecDeque<Task>,
    current: Option<Task>,
    /// The latest answer of a backlog search, reported when that search finishes.
    candidate: Option<PositionAnalysis>,
    paused: bool,
    /// The engine failed and could not be brought back; the next update tries again.
    failed: bool,
    /// The engine has been replaced once since the last update.
    recovered: bool,
}

impl<E: LiveEngine> Worker<E> {
    fn new(engine: E, sink: Arc<dyn LiveSink>, config: LiveConfig) -> Worker<E> {
        let game = Game::from_uci_moves(None, &[], BTreeMap::new())
            .expect("the starting position is valid");
        Worker {
            engine,
            sink,
            config,
            revision: 0,
            game,
            book_plies: 0,
            live_terminal: false,
            analyses: vec![None],
            sent: Vec::new(),
            tasks: VecDeque::new(),
            current: None,
            candidate: None,
            paused: true,
            failed: false,
            recovered: false,
        }
    }

    fn run(&mut self, inbox: &Receiver<Command>) {
        loop {
            let busy = self.current.is_some() || !self.tasks.is_empty();
            let command = if busy {
                match inbox.try_recv() {
                    Ok(command) => Some(command),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match inbox.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return,
                }
            };
            match command {
                Some(Command::SetMoves { revision, moves }) => self.set_moves(revision, moves),
                Some(Command::Pause) => {
                    self.paused = true;
                    self.interrupt();
                }
                Some(Command::Shutdown) => return,
                None => self.step(),
            }
        }
    }

    fn emit(&self, event: LiveEvent) {
        self.sink.emit(event);
    }

    fn fail(&mut self, error: &EngineError) {
        self.failed = true;
        self.current = None;
        self.tasks.clear();
        self.emit(LiveEvent::Error {
            revision: self.revision,
            message: error.to_string(),
        });
    }

    /// Replaces the engine, once per update. Returns whether the engine can be used.
    fn recover_or_fail(&mut self, error: &EngineError) -> bool {
        if self.recovered {
            self.fail(error);
            return false;
        }
        self.recovered = true;
        match self.engine.recover() {
            Ok(()) => true,
            Err(e) => {
                self.fail(&e);
                false
            }
        }
    }

    /// Stops whatever is running and forgets what was planned.
    fn interrupt(&mut self) -> bool {
        self.current = None;
        self.candidate = None;
        self.tasks.clear();
        match self.engine.stop() {
            Ok(()) => true,
            Err(e) => self.recover_or_fail(&e),
        }
    }

    fn set_moves(&mut self, revision: u64, moves: Vec<String>) {
        // Commands can reach the worker out of order; the newest revision is the truth.
        if revision < self.revision {
            return;
        }
        self.revision = revision;
        let was_paused = std::mem::replace(&mut self.paused, false);
        let was_failed = std::mem::replace(&mut self.failed, false);
        self.recovered = false;

        let same = self.game.moves.len() == moves.len()
            && self
                .game
                .moves
                .iter()
                .zip(&moves)
                .all(|(m, uci)| m.uci == *uci);
        if same && !was_paused && !was_failed {
            // Already working on exactly this; only tell the consumer again what is known.
            self.resend_known();
            return;
        }

        let game = match Game::from_uci_moves(None, &moves, BTreeMap::new()) {
            Ok(game) => game,
            Err(e) => {
                // Keep the old state (and its search); only report the refusal.
                self.paused = was_paused;
                self.failed = was_failed;
                self.emit(LiveEvent::Error {
                    revision,
                    message: e.to_string(),
                });
                return;
            }
        };

        let kept = self
            .game
            .moves
            .iter()
            .zip(&moves)
            .take_while(|(m, uci)| m.uci == **uci)
            .count();
        let usable = if was_failed {
            self.current = None;
            self.candidate = None;
            self.tasks.clear();
            match self.engine.recover() {
                Ok(()) => true,
                Err(e) => {
                    self.fail(&e);
                    false
                }
            }
        } else {
            self.interrupt()
        };

        self.analyses.truncate(kept + 1);
        self.analyses.resize(game.positions.len(), None);
        self.sent.truncate(kept);
        self.sent.resize(game.positions.len() - 1, None);
        self.book_plies = book_prefix(&game, self.config.book).0;
        self.game = game;
        let newest = self.game.moves.len();
        self.live_terminal = terminal_analysis(&self.game.position(newest)).is_some();

        // A finished game needs no engine to be scored.
        for index in 0..self.analyses.len() {
            if self.analyses[index].is_none()
                && let Some(terminal) = terminal_analysis(&self.game.position(index))
            {
                self.analyses[index] = Some(terminal);
            }
        }
        self.resend_known();
        if usable {
            self.plan();
        }
    }

    /// Queues the searches described at the top of the file.
    fn plan(&mut self) {
        self.tasks.clear();
        self.current = None;
        let newest = self.game.moves.len();
        let newest_depth = self.analyses[newest]
            .as_ref()
            .map_or(0, |a| a.lines[0].depth);
        if !self.live_terminal && newest_depth < QUICK_DEPTH {
            self.tasks.push_back(Task {
                index: newest,
                limit: SearchLimit::Depth(QUICK_DEPTH),
                stream: true,
            });
        }
        for index in (0..newest).rev() {
            if self.analyses[index].is_none() {
                self.tasks.push_back(Task {
                    index,
                    limit: SearchLimit::Depth(BACKLOG_DEPTH),
                    stream: false,
                });
            }
        }
        if !self.live_terminal {
            self.tasks.push_back(Task {
                index: newest,
                limit: SearchLimit::Infinite,
                stream: true,
            });
        }
    }

    /// Starts the next search, or reads what the running one has to say.
    fn step(&mut self) {
        if self.current.is_none() {
            let Some(task) = self.tasks.pop_front() else {
                return;
            };
            let fen = self.game.positions[task.index].clone();
            match self.engine.start(&fen, self.config.multipv, task.limit) {
                Ok(()) => {
                    self.candidate = None;
                    self.current = Some(task);
                }
                Err(e) => self.engine_failed(&e),
            }
            return;
        }
        match self.engine.poll(POLL_WAIT) {
            Ok(Some(update)) => self.apply(update),
            Ok(None) => {}
            Err(e) => self.engine_failed(&e),
        }
    }

    fn engine_failed(&mut self, error: &EngineError) {
        self.current = None;
        self.candidate = None;
        self.tasks.clear();
        if self.recover_or_fail(error) {
            self.plan();
        }
    }

    fn apply(&mut self, update: SearchUpdate) {
        let Some(task) = &self.current else {
            return;
        };
        let (index, stream) = (task.index, task.stream);
        match update {
            SearchUpdate::Depth(analysis) => {
                let depth = analysis.lines[0].depth;
                if !stream {
                    self.candidate = Some(analysis);
                    return;
                }
                let stored = self.analyses[index]
                    .as_ref()
                    .map_or(0, |a| a.lines[0].depth);
                if depth >= MIN_SHOWN_DEPTH && depth > stored {
                    self.commit(index, analysis);
                }
            }
            SearchUpdate::Finished => {
                if !stream && let Some(analysis) = self.candidate.take() {
                    self.commit(index, analysis);
                }
                self.current = None;
            }
        }
    }

    /// The event that reports `analysis` of position `index`.
    fn position_event(&self, index: usize, analysis: &PositionAnalysis) -> LiveEvent {
        let position = self.game.position(index);
        LiveEvent::Position {
            revision: self.revision,
            index,
            depth: analysis.lines[0].depth,
            lines: analysis
                .lines
                .iter()
                .map(|line| live_line(&position, line))
                .collect(),
        }
    }

    /// Stores an analysis, reports it, and re-classifies the moves it affects.
    fn commit(&mut self, index: usize, analysis: PositionAnalysis) {
        let event = self.position_event(index, &analysis);
        self.emit(event);
        self.analyses[index] = Some(analysis);
        self.reclassify();
    }

    /// Reports everything known about the game again, under the current revision. A consumer
    /// moves to a new revision the moment its user acts, so whatever this session sent under
    /// the old one in the meantime was dropped, and would never be sent again.
    fn resend_known(&mut self) {
        let events: Vec<LiveEvent> = self
            .analyses
            .iter()
            .enumerate()
            .filter_map(|(index, analysis)| {
                analysis
                    .as_ref()
                    .map(|analysis| self.position_event(index, analysis))
            })
            .collect();
        for event in events {
            self.emit(event);
        }
        self.sent.fill(None);
        self.reclassify();
    }

    /// Classifies every move whose two analyses exist and reports the ones that changed.
    fn reclassify(&mut self) {
        let newest = self.game.moves.len();
        let mut previous: Option<MoveClass> = None;
        for i in 0..newest {
            let (Some(before), Some(after)) = (&self.analyses[i], &self.analyses[i + 1]) else {
                previous = None;
                continue;
            };
            let review = review_move(
                i,
                &self.game,
                before,
                after,
                self.book_plies,
                previous,
                &self.config.thresholds,
            );
            previous = Some(review.class);
            let provisional =
                (i + 1 == newest && !self.live_terminal) || is_shallow(before) || is_shallow(after);
            let now = (review, provisional);
            if self.sent[i].as_ref() != Some(&now) {
                self.sink.emit(LiveEvent::Move {
                    revision: self.revision,
                    review: now.0.clone(),
                    provisional,
                });
                self.sent[i] = Some(now);
            }
        }
    }
}

/// A searched position that never got deep. (A finished game has depth 0 and is exact.)
fn is_shallow(analysis: &PositionAnalysis) -> bool {
    let depth = analysis.lines[0].depth;
    depth > 0 && depth <= BACKLOG_DEPTH
}

fn live_line(position: &Chess, line: &AnalysisLine) -> LiveLine {
    LiveLine {
        rank: line.rank,
        eval: line.eval,
        depth: line.depth,
        pv: line.pv.clone(),
        pv_san: pv_san(position, &line.pv),
    }
}

/// The SAN of a line of UCI moves, stopping at the first one that is not legal.
fn pv_san(start: &Chess, pv: &[String]) -> Vec<String> {
    let mut position = start.clone();
    let mut san = Vec::new();
    for uci in pv {
        let (Some(text), Some(next)) = (uci_to_san(&position, uci), apply_uci(&position, uci))
        else {
            break;
        };
        san.push(text);
        position = next;
    }
    san
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{LiveLog, ScriptedLiveEngine};
    use crate::review::{ReviewOptions, review_game};
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Condvar, Mutex};
    use std::time::Instant;

    /// The depths the scripted engine reports for every position, like a real search deepening.
    const LADDER: [u32; 5] = [4, 8, 12, 16, 20];

    const E4: &str = "e2e4";
    const E5: &str = "e7e5";
    const NF3: &str = "g1f3";

    fn leaked_empty_book() -> &'static OpeningBook {
        Box::leak(Box::new(OpeningBook::empty()))
    }

    fn config() -> LiveConfig {
        LiveConfig {
            multipv: 3,
            thresholds: Thresholds::default(),
            book: leaked_empty_book(),
        }
    }

    /// The FEN after playing `moves` from the start.
    fn fen_after(moves: &[&str]) -> String {
        let moves: Vec<String> = moves.iter().map(|m| m.to_string()).collect();
        let game = Game::from_uci_moves(None, &moves, BTreeMap::new()).unwrap();
        game.positions.last().unwrap().clone()
    }

    fn moves(list: &[&str]) -> Vec<String> {
        list.iter().map(|m| m.to_string()).collect()
    }

    /// What the scripted engine thinks of each position; anything not listed is equal.
    #[derive(Default, Clone)]
    struct Opinions(HashMap<String, (Eval, String)>);

    impl Opinions {
        fn with(mut self, after: &[&str], eval: Eval, best: &str) -> Opinions {
            self.0.insert(fen_after(after), (eval, best.to_string()));
            self
        }

        fn analyses(&self, fen: &str) -> Vec<PositionAnalysis> {
            let (eval, best) = self
                .0
                .get(fen)
                .cloned()
                .unwrap_or((Eval::Cp(0), "a2a3".to_string()));
            LADDER
                .iter()
                .map(|&depth| PositionAnalysis {
                    lines: vec![AnalysisLine {
                        rank: 1,
                        eval,
                        depth,
                        pv: vec![best.clone()],
                    }],
                })
                .collect()
        }

        fn engine(&self) -> ScriptedLiveEngine {
            let opinions = self.clone();
            ScriptedLiveEngine::new(move |fen| opinions.analyses(fen))
        }
    }

    #[derive(Default)]
    struct Collect {
        events: Mutex<Vec<LiveEvent>>,
        changed: Condvar,
    }

    impl LiveSink for Collect {
        fn emit(&self, event: LiveEvent) {
            self.events.lock().unwrap().push(event);
            self.changed.notify_all();
        }
    }

    impl Collect {
        fn events(&self) -> Vec<LiveEvent> {
            self.events.lock().unwrap().clone()
        }

        /// Waits (up to ten seconds) until `done` accepts the events so far.
        fn wait_for(&self, what: &str, done: impl Fn(&[LiveEvent]) -> bool) -> Vec<LiveEvent> {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut events = self.events.lock().unwrap();
            while !done(&events) {
                let left = deadline.saturating_duration_since(Instant::now());
                assert!(
                    !left.is_zero(),
                    "timed out waiting for {what}; saw {events:#?}"
                );
                events = self.changed.wait_timeout(events, left).unwrap().0;
            }
            events.clone()
        }
    }

    fn position_at(events: &[LiveEvent], index: usize, min_depth: u32) -> bool {
        events.iter().any(|e| {
            matches!(e, LiveEvent::Position { index: i, depth, .. } if *i == index && *depth >= min_depth)
        })
    }

    fn last_move(events: &[LiveEvent], ply: usize) -> Option<(MoveReview, bool)> {
        events.iter().rev().find_map(|e| match e {
            LiveEvent::Move {
                review,
                provisional,
                ..
            } if review.ply == ply => Some((review.clone(), *provisional)),
            _ => None,
        })
    }

    struct Rig {
        session: LiveSession,
        sink: Arc<Collect>,
        log: Arc<Mutex<LiveLog>>,
    }

    fn rig(opinions: &Opinions) -> Rig {
        let engine = opinions.engine();
        let log = engine.log();
        let sink = Arc::new(Collect::default());
        let session = LiveSession::start(engine, sink.clone(), config());
        Rig { session, sink, log }
    }

    impl Rig {
        fn starts(&self) -> Vec<(String, SearchLimit)> {
            self.log
                .lock()
                .unwrap()
                .starts
                .iter()
                .map(|(fen, _, limit)| (fen.clone(), *limit))
                .collect()
        }
    }

    #[test]
    fn a_first_move_gets_a_quick_answer_then_a_deepening_one_and_a_class() {
        let opinions =
            Opinions::default()
                .with(&[], Eval::Cp(30), E4)
                .with(&[E4], Eval::Cp(30), E5);
        let rig = rig(&opinions);
        rig.session.set_moves(1, moves(&[E4]));
        let events = rig
            .sink
            .wait_for("the deep answer", |e| position_at(e, 1, 20));

        let (review, provisional) = last_move(&events, 1).expect("the move is classified");
        assert_eq!(review.class, MoveClass::Best);
        assert!(provisional, "the newest move is provisional");
        assert!(events.iter().all(|e| match e {
            LiveEvent::Position { revision, .. }
            | LiveEvent::Move { revision, .. }
            | LiveEvent::Error { revision, .. } => *revision == 1,
        }));

        // The newest position first, quickly; then the one never analysed; then it deepens.
        assert_eq!(
            rig.starts(),
            [
                (fen_after(&[E4]), SearchLimit::Depth(QUICK_DEPTH)),
                (fen_after(&[]), SearchLimit::Depth(BACKLOG_DEPTH)),
                (fen_after(&[E4]), SearchLimit::Infinite),
            ]
        );
    }

    #[test]
    fn depths_too_shallow_to_trust_are_not_shown() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        let events = rig
            .sink
            .wait_for("the deep answer", |e| position_at(e, 1, 20));
        for event in &events {
            if let LiveEvent::Position { depth, .. } = event {
                assert!(*depth >= MIN_SHOWN_DEPTH, "depth {depth} was shown");
            }
        }
    }

    #[test]
    fn a_batch_of_moves_gets_the_deep_search_only_on_the_last_position() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4, E5, NF3]));
        let events = rig
            .sink
            .wait_for("the deep answer", |e| position_at(e, 3, 20));

        assert_eq!(
            rig.starts(),
            [
                (fen_after(&[E4, E5, NF3]), SearchLimit::Depth(QUICK_DEPTH)),
                (fen_after(&[E4, E5]), SearchLimit::Depth(BACKLOG_DEPTH)),
                (fen_after(&[E4]), SearchLimit::Depth(BACKLOG_DEPTH)),
                (fen_after(&[]), SearchLimit::Depth(BACKLOG_DEPTH)),
                (fen_after(&[E4, E5, NF3]), SearchLimit::Infinite),
            ]
        );
        // The backlog is only searched to the shallow depth, so its moves stay provisional.
        for ply in 1..=3 {
            let (_, provisional) = last_move(&events, ply).expect("every move has a class");
            assert!(provisional, "ply {ply}");
        }
        assert!(position_at(&events, 0, BACKLOG_DEPTH));
        assert!(!position_at(&events, 0, BACKLOG_DEPTH + 1));
    }

    #[test]
    fn a_new_move_settles_the_one_before_it() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));
        rig.session.set_moves(2, moves(&[E4, E5]));
        rig.sink.wait_for("move 2 deep", |e| position_at(e, 2, 20));
        rig.session.set_moves(3, moves(&[E4, E5, NF3]));
        let events = rig.sink.wait_for("move 3 deep", |e| position_at(e, 3, 20));

        let (_, newest) = last_move(&events, 3).unwrap();
        assert!(newest, "the newest move is provisional");
        let (_, settled) = last_move(&events, 2).unwrap();
        assert!(
            !settled,
            "move 2 had deep analyses on both sides and is no longer the newest"
        );
        // Only the newest position was searched each time: the older ones were kept.
        let infinite = rig
            .starts()
            .into_iter()
            .filter(|(_, limit)| *limit == SearchLimit::Infinite)
            .count();
        assert_eq!(infinite, 3);
    }

    #[test]
    fn taking_a_move_back_keeps_what_is_known_about_the_remaining_positions() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));
        rig.session.set_moves(2, moves(&[E4, E5]));
        rig.sink.wait_for("move 2 deep", |e| position_at(e, 2, 20));
        let before = rig.starts().len();

        rig.session.set_moves(3, moves(&[E4]));
        // Searching again starts straight away at no depth limit: position 1 is already deep.
        let deadline = Instant::now() + Duration::from_secs(10);
        while rig.starts().len() == before {
            assert!(Instant::now() < deadline, "no search was started");
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(100));
        let after: Vec<_> = rig.starts().split_off(before);
        assert_eq!(after, [(fen_after(&[E4]), SearchLimit::Infinite)]);

        let later: Vec<LiveEvent> = rig
            .sink
            .events()
            .into_iter()
            .filter(|e| matches!(e, LiveEvent::Position { revision: 3, .. }))
            .collect();
        // The kept positions are sent again exactly as they were, and the search that restarts
        // finds nothing deeper than the stored 20 to replace them with.
        let depths_of = |index: usize| -> Vec<u32> {
            later
                .iter()
                .filter_map(|e| match e {
                    LiveEvent::Position {
                        index: i, depth, ..
                    } if *i == index => Some(*depth),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(depths_of(1), [20], "{later:?}");
        assert!(
            depths_of(2).is_empty(),
            "the taken-back position is gone: {later:?}"
        );
    }

    #[test]
    fn a_different_move_after_a_take_back_replaces_the_old_line() {
        let opinions = Opinions::default()
            .with(&[E4, E5], Eval::Cp(-25), NF3)
            .with(&[E4, "c7c5"], Eval::Cp(-10), NF3);
        let rig = rig(&opinions);
        rig.session.set_moves(1, moves(&[E4, E5]));
        rig.sink.wait_for("first line", |e| position_at(e, 2, 20));
        rig.session.set_moves(2, moves(&[E4, "c7c5"]));
        let events = rig.sink.wait_for("second line", |e| {
            e.iter().any(|ev| {
                matches!(
                    ev,
                    LiveEvent::Position {
                        revision: 2,
                        index: 2,
                        depth: 20,
                        ..
                    }
                )
            })
        });

        let (review, _) = last_move(&events, 2).unwrap();
        assert_eq!(review.uci, "c7c5");
        assert_eq!(review.eval_after, Eval::Cp(-10));
    }

    #[test]
    fn a_bad_move_is_classified_the_way_a_finished_review_classifies_it() {
        let opinions =
            Opinions::default()
                .with(&[], Eval::Cp(30), E4)
                .with(&["f2f3"], Eval::Cp(-300), E5);
        let rig = rig(&opinions);
        rig.session.set_moves(1, moves(&["f2f3"]));
        let events = rig
            .sink
            .wait_for("the deep answer", |e| position_at(e, 1, 20));

        let (review, _) = last_move(&events, 1).unwrap();
        assert_eq!(review.class, MoveClass::Blunder);
        assert_eq!(review.best_uci.as_deref(), Some(E4));
        assert_eq!(review.best_san.as_deref(), Some("e4"));
    }

    #[test]
    fn live_classes_match_a_finished_review_of_the_same_analyses() {
        let line = [E4, E5, NF3];
        let opinions = Opinions::default()
            .with(&[], Eval::Cp(30), E4)
            .with(&[E4], Eval::Cp(30), E5)
            .with(&[E4, E5], Eval::Cp(40), NF3)
            .with(&[E4, E5, NF3], Eval::Cp(-200), "b8c6");
        let rig = rig(&opinions);
        rig.session.set_moves(1, moves(&line));
        let events = rig
            .sink
            .wait_for("the deep answer", |e| position_at(e, 3, 20));

        let game = Game::from_uci_moves(None, &moves(&line), BTreeMap::new()).unwrap();
        let script: Vec<PositionAnalysis> = game
            .positions
            .iter()
            .map(|fen| opinions.analyses(fen).pop().unwrap())
            .collect();
        let mut analyzer = crate::engine::ScriptedAnalyzer::new(script);
        let review = review_game(
            &game,
            &mut analyzer,
            &ReviewOptions::default(),
            leaked_empty_book(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();

        for expected in &review.moves {
            let (live, _) = last_move(&events, expected.ply).unwrap();
            assert_eq!(&live, expected, "ply {}", expected.ply);
        }
    }

    #[test]
    fn lines_carry_their_moves_in_san() {
        let start = Chess::default();
        let pv = moves(&[E4, E5, NF3, "b8c6", "f1b5"]);
        assert_eq!(pv_san(&start, &pv), ["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        // An illegal move ends the line instead of failing.
        assert_eq!(pv_san(&start, &moves(&[E4, "e2e4", NF3])), ["e4"]);
        assert!(pv_san(&start, &[]).is_empty());
    }

    #[test]
    fn a_finished_game_is_scored_without_the_engine() {
        let fools_mate = ["f2f3", "e7e5", "g2g4", "d8h4"];
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&fools_mate));
        let events = rig
            .sink
            .wait_for("the mating move", |e| last_move(e, 4).is_some());

        let mated = fen_after(&fools_mate);
        assert!(rig.starts().iter().all(|(fen, _)| *fen != mated));
        assert!(events.iter().any(|e| matches!(
            e,
            LiveEvent::Position { index: 4, depth: 0, lines, .. }
                if lines[0].eval == Eval::Checkmate(crate::eval::Side::Black)
        )));
        let (mating, _) = last_move(&events, 4).unwrap();
        assert_eq!(mating.san, "Qh4#");
        assert_eq!(mating.eval_after, Eval::Checkmate(crate::eval::Side::Black));
    }

    #[test]
    fn an_illegal_move_list_is_refused_and_the_search_carries_on() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));

        rig.session.set_moves(2, moves(&[E4, "e2e5"]));
        let events = rig.sink.wait_for("the refusal", |e| {
            e.iter()
                .any(|ev| matches!(ev, LiveEvent::Error { revision: 2, .. }))
        });
        let message = events
            .iter()
            .find_map(|e| match e {
                LiveEvent::Error { message, .. } => Some(message.clone()),
                _ => None,
            })
            .unwrap();
        assert!(message.contains("e2e5"), "{message}");

        // A valid list afterwards works as normal.
        rig.session.set_moves(3, moves(&[E4, E5]));
        rig.sink.wait_for("move 2 deep", |e| position_at(e, 2, 20));
    }

    #[test]
    fn pausing_stops_the_search_and_the_next_update_resumes_it() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));
        let stops = rig.log.lock().unwrap().stops;
        let starts = rig.starts().len();

        rig.session.pause();
        let deadline = Instant::now() + Duration::from_secs(10);
        while rig.log.lock().unwrap().stops == stops {
            assert!(Instant::now() < deadline, "the engine was never stopped");
            std::thread::sleep(Duration::from_millis(5));
        }

        rig.session.set_moves(2, moves(&[E4]));
        while rig.starts().len() == starts {
            assert!(Instant::now() < deadline, "the search never resumed");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            rig.starts().last().unwrap(),
            &(fen_after(&[E4]), SearchLimit::Infinite)
        );
    }

    #[test]
    fn repeating_the_same_moves_does_not_restart_the_search() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));
        let starts = rig.starts().len();

        rig.session.set_moves(2, moves(&[E4]));
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(rig.starts().len(), starts);
    }

    #[test]
    fn a_failing_engine_is_replaced_once_and_the_search_goes_on() {
        let rig = rig(&Opinions::default());
        rig.log.lock().unwrap().fail_polls = 1;
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink
            .wait_for("the deep answer", |e| position_at(e, 1, 20));
        assert_eq!(rig.log.lock().unwrap().recoveries, 1);
        assert!(
            rig.sink
                .events()
                .iter()
                .all(|e| !matches!(e, LiveEvent::Error { .. }))
        );
    }

    #[test]
    fn an_engine_that_keeps_failing_is_reported_and_tried_again_on_the_next_update() {
        let rig = rig(&Opinions::default());
        rig.log.lock().unwrap().fail_polls = 1_000;
        rig.session.set_moves(1, moves(&[E4]));
        let events = rig.sink.wait_for("the failure", |e| {
            e.iter().any(|ev| matches!(ev, LiveEvent::Error { .. }))
        });
        let message = events
            .iter()
            .find_map(|e| match e {
                LiveEvent::Error { message, .. } => Some(message.clone()),
                _ => None,
            })
            .unwrap();
        assert!(message.contains("scripted failure"), "{message}");

        // The engine comes back; asking again (even for the same moves) restarts the analysis.
        rig.log.lock().unwrap().fail_polls = 0;
        rig.session.set_moves(2, moves(&[E4]));
        rig.sink
            .wait_for("the deep answer", |e| position_at(e, 1, 20));
        assert_eq!(rig.log.lock().unwrap().recoveries, 2);
    }

    #[test]
    fn an_engine_that_cannot_be_replaced_is_reported() {
        let rig = rig(&Opinions::default());
        {
            let mut log = rig.log.lock().unwrap();
            log.fail_polls = 1;
            log.fail_recover = true;
        }
        rig.session.set_moves(1, moves(&[E4]));
        let events = rig.sink.wait_for("the failure", |e| {
            e.iter().any(|ev| matches!(ev, LiveEvent::Error { .. }))
        });
        assert!(events.iter().any(|e| matches!(
            e,
            LiveEvent::Error { message, .. } if message.contains("could not start the engine")
        )));
    }

    #[test]
    fn a_panic_in_the_session_is_reported() {
        let engine = ScriptedLiveEngine::new(|_| panic!("the script blew up"));
        let sink = Arc::new(Collect::default());
        let session = LiveSession::start(engine, sink.clone(), config());
        session.set_moves(5, moves(&[E4]));
        let events = sink.wait_for("the report", |e| !e.is_empty());
        assert!(matches!(
            &events[0],
            LiveEvent::Error { revision: 5, message } if message.contains("unexpectedly")
        ));
    }

    #[test]
    fn dropping_the_session_ends_its_thread_and_releases_the_engine() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));
        let Rig { session, sink, .. } = rig;
        assert!(Arc::strong_count(&sink) > 1, "the thread holds the sink");
        drop(session);
        assert_eq!(Arc::strong_count(&sink), 1);
    }

    #[test]
    fn a_new_update_resends_what_it_keeps_under_its_own_revision() {
        // Anything sent under the old revision after the consumer moved on was dropped by it,
        // and nothing would send it again; so what is kept is sent again.
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));

        rig.session.set_moves(2, moves(&[E4, E5]));
        let events = rig.sink.wait_for("the kept positions and move again", |e| {
            let again = |index: usize, depth: u32| {
                e.iter().any(|ev| {
                    matches!(ev, LiveEvent::Position { revision: 2, index: i, depth: d, .. }
                        if *i == index && *d >= depth)
                })
            };
            again(0, 12)
                && again(1, 20)
                && e.iter().any(|ev| {
                    matches!(ev, LiveEvent::Move { revision: 2, review, .. } if review.ply == 1)
                })
        });
        assert!(
            !events
                .iter()
                .any(|ev| matches!(ev, LiveEvent::Error { .. })),
            "{events:#?}"
        );
    }

    #[test]
    fn repeating_the_same_moves_resends_what_is_known_under_the_new_revision() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));
        let starts = rig.starts().len();

        rig.session.set_moves(2, moves(&[E4]));
        rig.sink.wait_for("the position again", |e| {
            e.iter().any(|ev| {
                matches!(
                    ev,
                    LiveEvent::Position {
                        revision: 2,
                        index: 1,
                        depth: 20,
                        ..
                    }
                )
            })
        });
        assert_eq!(rig.starts().len(), starts, "nothing is searched again");
    }

    #[test]
    fn an_update_from_an_older_revision_is_ignored() {
        // Commands can overtake each other on the way here; the newest revision must win.
        let rig = rig(&Opinions::default());
        rig.session.set_moves(2, moves(&[E4, E5]));
        rig.sink.wait_for("move 2 deep", |e| position_at(e, 2, 20));
        let starts = rig.starts().len();

        rig.session.set_moves(1, moves(&[E4]));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            rig.starts().len(),
            starts,
            "an older update must not start a search"
        );
        let revision_of = |e: &LiveEvent| match e {
            LiveEvent::Position { revision, .. }
            | LiveEvent::Move { revision, .. }
            | LiveEvent::Error { revision, .. } => *revision,
        };
        assert!(rig.sink.events().iter().all(|e| revision_of(e) == 2));
    }

    #[test]
    fn a_session_says_whether_its_worker_is_still_running() {
        let rig = rig(&Opinions::default());
        rig.session.set_moves(1, moves(&[E4]));
        rig.sink.wait_for("move 1 deep", |e| position_at(e, 1, 20));
        assert!(rig.session.is_running());

        // A session whose worker panicked cannot be asked anything any more; the shell needs
        // to see that, so that "restart" builds a new one instead of writing to a dead channel.
        let engine = ScriptedLiveEngine::new(|_| panic!("the script blew up"));
        let sink = Arc::new(Collect::default());
        let dead = LiveSession::start(engine, sink.clone(), config());
        dead.set_moves(1, moves(&[E4]));
        sink.wait_for("the report", |e| !e.is_empty());
        let deadline = Instant::now() + Duration::from_secs(10);
        while dead.is_running() {
            assert!(Instant::now() < deadline, "the worker never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn events_serialize_for_the_frontend() {
        let event = LiveEvent::Error {
            revision: 4,
            message: "no".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"kind":"error","revision":4,"message":"no"}"#
        );
    }

    #[test]
    fn known_opening_moves_stay_book_while_live() {
        let engine = Opinions::default().engine();
        let sink = Arc::new(Collect::default());
        let config = LiveConfig {
            book: OpeningBook::bundled(),
            ..config()
        };
        let session = LiveSession::start(engine, sink.clone(), config);
        session.set_moves(1, moves(&[E4, E5]));
        let events = sink.wait_for("the deep answer", |e| position_at(e, 2, 20));
        assert_eq!(last_move(&events, 1).unwrap().0.class, MoveClass::Book);
        assert_eq!(last_move(&events, 2).unwrap().0.class, MoveClass::Book);
    }
}
