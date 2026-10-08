//! Stockfish integration over UCI.
//!
//! `Analyzer` is the seam the rest of the crate depends on. `UciEngine` is the real
//! implementation; `ScriptedAnalyzer` is a canned one for tests.
//!
//! `LiveEngine` is the second seam, for a search that keeps running until it is interrupted
//! (following a game as it is played). `UciEngine` implements both; `ScriptedLiveEngine` is
//! the canned one for tests.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use crate::eval::{Eval, Side, UciScore};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("Stockfish was not found; run scripts/setup-stockfish or set the engine path")]
    NotFound,
    #[error("could not start the engine: {0}")]
    Spawn(String),
    #[error("the engine did not answer within {0:?}")]
    Timeout(Duration),
    #[error("unexpected engine output: {0}")]
    Protocol(String),
    #[error("the engine returned no analysis for {0}")]
    NoAnalysis(String),
    #[error("engine I/O failed: {0}")]
    Io(String),
}

/// Search limits for one analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Limits {
    pub depth: u32,
    pub multipv: u32,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            depth: 20,
            multipv: 3,
        }
    }
}

/// One principal variation reported by the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisLine {
    /// 1 = best line.
    pub rank: u32,
    pub eval: Eval,
    pub depth: u32,
    /// Moves in UCI notation. The first move is the line's move.
    pub pv: Vec<String>,
}

/// The engine's analysis of one position; `lines` is sorted by rank and never empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionAnalysis {
    pub lines: Vec<AnalysisLine>,
}

pub trait Analyzer {
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError>;
    /// Identifies the engine and version; part of the cache key.
    fn engine_id(&self) -> String;
}

impl<T: Analyzer + ?Sized> Analyzer for Box<T> {
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        (**self).analyze(fen, limits)
    }

    fn engine_id(&self) -> String {
        (**self).engine_id()
    }
}

/// How long a live search runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchLimit {
    Depth(u32),
    /// Until the search is stopped or replaced.
    Infinite,
}

/// What a running search reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchUpdate {
    /// Every line of one finished depth, sorted by rank. All lines share one depth.
    Depth(PositionAnalysis),
    /// A depth-limited search ended on its own. An infinite search never sends this.
    Finished,
}

/// An engine that searches until told to stop, reporting as it deepens.
pub trait LiveEngine {
    /// Starts searching `fen`. A search in progress is stopped first.
    fn start(&mut self, fen: &str, multipv: u32, limit: SearchLimit) -> Result<(), EngineError>;
    /// The next update, or `None` if nothing arrived within `wait` (or nothing is running).
    fn poll(&mut self, wait: Duration) -> Result<Option<SearchUpdate>, EngineError>;
    /// Stops the current search and returns once the engine is idle. Updates not yet
    /// polled are discarded, so none of them can be mistaken for the next search's.
    fn stop(&mut self) -> Result<(), EngineError>;
    /// Replaces a dead or stuck engine process with a fresh one.
    fn recover(&mut self) -> Result<(), EngineError>;
}

impl<T: LiveEngine + ?Sized> LiveEngine for Box<T> {
    fn start(&mut self, fen: &str, multipv: u32, limit: SearchLimit) -> Result<(), EngineError> {
        (**self).start(fen, multipv, limit)
    }

    fn poll(&mut self, wait: Duration) -> Result<Option<SearchUpdate>, EngineError> {
        (**self).poll(wait)
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        (**self).stop()
    }

    fn recover(&mut self) -> Result<(), EngineError> {
        (**self).recover()
    }
}

pub fn side_to_move(fen: &str) -> Side {
    if fen.split(' ').nth(1) == Some("b") {
        Side::Black
    } else {
        Side::White
    }
}

/// Parses a UCI `info` line into an analysis line (White's point of view).
/// Returns `None` for lines without a score and principal variation, and for
/// bound-only scores (`lowerbound` / `upperbound`).
pub fn parse_info_line(line: &str, stm: Side) -> Option<AnalysisLine> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "info" {
        return None;
    }
    let tokens: Vec<&str> = tokens.collect();
    let mut depth = None;
    let mut rank = 1;
    let mut score = None;
    let mut pv = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                depth = tokens.get(i + 1)?.parse::<u32>().ok();
                i += 2;
            }
            "multipv" => {
                rank = tokens.get(i + 1)?.parse::<u32>().ok()?;
                i += 2;
            }
            "score" => {
                let kind = *tokens.get(i + 1)?;
                let value = tokens.get(i + 2)?.parse::<i32>().ok()?;
                score = match kind {
                    "cp" => Some(UciScore::Cp(value)),
                    "mate" => Some(UciScore::Mate(value)),
                    _ => return None,
                };
                i += 3;
                if matches!(tokens.get(i), Some(&"lowerbound") | Some(&"upperbound")) {
                    return None;
                }
            }
            "pv" => {
                pv = tokens[i + 1..].iter().map(|s| s.to_string()).collect();
                break;
            }
            _ => i += 1,
        }
    }
    Some(AnalysisLine {
        rank,
        eval: Eval::from_uci(score?, stm),
        depth: depth?,
        pv,
    })
    .filter(|l| !l.pv.is_empty())
}

fn engine_file_name() -> &'static str {
    if cfg!(windows) {
        "stockfish.exe"
    } else {
        "stockfish"
    }
}

/// Looks for `engines/<name>` in `start` and every parent directory.
fn find_in_engines_dir(start: &Path, name: &str) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let candidate = d.join("engines").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

/// Finds a Stockfish executable: the explicit path, then `STOCKFISH_PATH`, then one next to
/// the running program (where a bundled copy sits), then `engines/stockfish[.exe]` in the
/// program's directory or any parent, and finally in the current directory or any parent.
pub fn locate_stockfish(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return path.is_file().then(|| path.to_path_buf());
    }
    if let Ok(path) = std::env::var("STOCKFISH_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let name = engine_file_name();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        let beside = dir.join(name);
        if beside.is_file() {
            return Some(beside);
        }
        if let Some(found) = find_in_engines_dir(&dir, name) {
            return Some(found);
        }
    }
    std::env::current_dir()
        .ok()
        .and_then(|cwd| find_in_engines_dir(&cwd, name))
}

/// How long the engine gets to start and answer the UCI handshake.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub path: PathBuf,
    pub threads: u32,
    pub hash_mb: u32,
    /// Maximum time to wait for any single analysis (starting the process has its own, longer limit).
    pub timeout: Duration,
}

impl EngineConfig {
    pub fn new(path: PathBuf) -> EngineConfig {
        EngineConfig {
            path,
            threads: 1,
            hash_mb: 256,
            timeout: Duration::from_secs(120),
        }
    }
}

/// Stockfish is a console program. The desktop app in a release build has no console of its
/// own, so on Windows it would open a visible console window for every engine it starts, and
/// closing that window would kill the engine mid-review.
#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

/// A running Stockfish process.
pub struct UciEngine {
    config: EngineConfig,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    name: String,
}

impl UciEngine {
    pub fn start(config: EngineConfig) -> Result<UciEngine, EngineError> {
        if !config.path.is_file() {
            return Err(EngineError::NotFound);
        }
        let mut command = Command::new(&config.path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hide_console_window(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| EngineError::Spawn(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EngineError::Spawn("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Spawn("no stdout".into()))?;
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut engine = UciEngine {
            config,
            child,
            stdin,
            lines,
            name: String::new(),
        };
        engine.handshake()?;
        Ok(engine)
    }

    fn handshake(&mut self) -> Result<(), EngineError> {
        self.send("uci")?;
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            let line = self.next_line(deadline, STARTUP_TIMEOUT)?;
            if let Some(name) = line.strip_prefix("id name ") {
                self.name = name.trim().to_string();
            }
            if line.trim() == "uciok" {
                break;
            }
        }
        self.send(&format!(
            "setoption name Threads value {}",
            self.config.threads
        ))?;
        self.send(&format!(
            "setoption name Hash value {}",
            self.config.hash_mb
        ))?;
        self.send("isready")?;
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while self.next_line(deadline, STARTUP_TIMEOUT)?.trim() != "readyok" {}
        Ok(())
    }

    fn send(&mut self, command: &str) -> Result<(), EngineError> {
        writeln!(self.stdin, "{command}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| EngineError::Io(e.to_string()))
    }

    fn next_line(&mut self, deadline: Instant, limit: Duration) -> Result<String, EngineError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self.lines.recv_timeout(remaining) {
            Ok(line) => Ok(line),
            Err(RecvTimeoutError::Timeout) => Err(EngineError::Timeout(limit)),
            Err(RecvTimeoutError::Disconnected) => Err(EngineError::Io("engine exited".into())),
        }
    }

    fn try_analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        let stm = side_to_move(fen);
        self.send(&format!("setoption name MultiPV value {}", limits.multipv))?;
        self.send(&format!("position fen {fen}"))?;
        self.send(&format!("go depth {}", limits.depth))?;
        let deadline = Instant::now() + self.config.timeout;
        let mut best: BTreeMap<u32, AnalysisLine> = BTreeMap::new();
        loop {
            let line = self.next_line(deadline, self.config.timeout)?;
            if line.starts_with("bestmove") {
                break;
            }
            if let Some(parsed) = parse_info_line(&line, stm) {
                best.insert(parsed.rank, parsed);
            }
        }
        if best.is_empty() {
            return Err(EngineError::NoAnalysis(fen.to_string()));
        }
        Ok(PositionAnalysis {
            lines: best.into_values().collect(),
        })
    }

    fn restart(&mut self) -> Result<(), EngineError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let fresh = UciEngine::start(self.config.clone())?;
        *self = fresh;
        Ok(())
    }
}

impl Analyzer for UciEngine {
    /// Analyses a position. On a timeout or a dead process the engine is restarted
    /// and the position retried once.
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        match self.try_analyze(fen, limits) {
            Err(EngineError::Timeout(_) | EngineError::Io(_)) => {
                self.restart()?;
                self.try_analyze(fen, limits)
            }
            other => other,
        }
    }

    fn engine_id(&self) -> String {
        self.name.clone()
    }
}

impl Drop for UciEngine {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// An `Analyzer` that returns canned answers in order. For tests.
pub struct ScriptedAnalyzer {
    responses: VecDeque<PositionAnalysis>,
    pub calls: usize,
}

impl ScriptedAnalyzer {
    pub fn new(responses: Vec<PositionAnalysis>) -> ScriptedAnalyzer {
        ScriptedAnalyzer {
            responses: responses.into(),
            calls: 0,
        }
    }
}

impl Analyzer for ScriptedAnalyzer {
    fn analyze(&mut self, fen: &str, _limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        self.calls += 1;
        self.responses
            .pop_front()
            .ok_or_else(|| EngineError::NoAnalysis(fen.to_string()))
    }

    fn engine_id(&self) -> String {
        "scripted".to_string()
    }
}

/// What a `ScriptedLiveEngine` was asked to do, shared so a test can look after the engine
/// has moved onto another thread. The `fail_*` fields make the engine misbehave on demand.
#[derive(Debug, Default)]
pub struct LiveLog {
    pub starts: Vec<(String, u32, SearchLimit)>,
    pub stops: usize,
    pub recoveries: usize,
    /// The next this-many `poll` calls fail.
    pub fail_polls: usize,
    /// `recover` fails while this is set.
    pub fail_recover: bool,
}

type LiveScript = dyn FnMut(&str) -> Vec<PositionAnalysis> + Send;

/// A `LiveEngine` that answers from a script. For tests.
///
/// `script` maps a position to the analyses the engine reports for it, one per depth, in order.
/// A depth-limited search reports those up to its depth and then finishes; an infinite search
/// reports all of them and then stays silent, like Stockfish.
pub struct ScriptedLiveEngine {
    script: Box<LiveScript>,
    queue: VecDeque<SearchUpdate>,
    log: Arc<Mutex<LiveLog>>,
}

impl ScriptedLiveEngine {
    pub fn new(
        script: impl FnMut(&str) -> Vec<PositionAnalysis> + Send + 'static,
    ) -> ScriptedLiveEngine {
        ScriptedLiveEngine {
            script: Box::new(script),
            queue: VecDeque::new(),
            log: Arc::new(Mutex::new(LiveLog::default())),
        }
    }

    /// A handle on what this engine is asked to do.
    pub fn log(&self) -> Arc<Mutex<LiveLog>> {
        Arc::clone(&self.log)
    }

    fn record<T>(&self, change: impl FnOnce(&mut LiveLog) -> T) -> T {
        change(&mut self.log.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl LiveEngine for ScriptedLiveEngine {
    fn start(&mut self, fen: &str, multipv: u32, limit: SearchLimit) -> Result<(), EngineError> {
        self.queue.clear();
        self.record(|log| log.starts.push((fen.to_string(), multipv, limit)));
        for analysis in (self.script)(fen) {
            let depth = analysis.lines[0].depth;
            if matches!(limit, SearchLimit::Depth(max) if depth > max) {
                break;
            }
            self.queue.push_back(SearchUpdate::Depth(analysis));
        }
        if matches!(limit, SearchLimit::Depth(_)) {
            self.queue.push_back(SearchUpdate::Finished);
        }
        Ok(())
    }

    fn poll(&mut self, wait: Duration) -> Result<Option<SearchUpdate>, EngineError> {
        let failing = self.record(|log| {
            let failing = log.fail_polls > 0;
            log.fail_polls = log.fail_polls.saturating_sub(1);
            failing
        });
        if failing {
            return Err(EngineError::Io("scripted failure".into()));
        }
        if let Some(update) = self.queue.pop_front() {
            return Ok(Some(update));
        }
        std::thread::sleep(wait.min(Duration::from_millis(2)));
        Ok(None)
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        self.queue.clear();
        self.record(|log| log.stops += 1);
        Ok(())
    }

    fn recover(&mut self) -> Result<(), EngineError> {
        self.queue.clear();
        self.record(|log| {
            log.recoveries += 1;
            if log.fail_recover {
                Err(EngineError::Spawn("scripted failure".into()))
            } else {
                Ok(())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_centipawn_line() {
        let line = "info depth 20 seldepth 28 multipv 2 score cp 34 nodes 1000 nps 5000 time 200 pv e2e4 e7e5 g1f3";
        let parsed = parse_info_line(line, Side::White).unwrap();
        assert_eq!(parsed.rank, 2);
        assert_eq!(parsed.depth, 20);
        assert_eq!(parsed.eval, Eval::Cp(34));
        assert_eq!(parsed.pv, ["e2e4", "e7e5", "g1f3"]);
    }

    #[test]
    fn black_to_move_flips_the_score() {
        let line = "info depth 12 multipv 1 score cp 40 pv e7e5";
        assert_eq!(
            parse_info_line(line, Side::Black).unwrap().eval,
            Eval::Cp(-40)
        );
    }

    #[test]
    fn parses_mate_scores() {
        let line = "info depth 18 multipv 1 score mate 3 pv d8h4";
        assert_eq!(
            parse_info_line(line, Side::White).unwrap().eval,
            Eval::Mate(3)
        );
        let line = "info depth 18 multipv 1 score mate -2 pv d8h4";
        assert_eq!(
            parse_info_line(line, Side::White).unwrap().eval,
            Eval::Mate(-2)
        );
    }

    #[test]
    fn multipv_defaults_to_one() {
        let line = "info depth 5 score cp 10 pv e2e4";
        assert_eq!(parse_info_line(line, Side::White).unwrap().rank, 1);
    }

    #[test]
    fn ignores_lines_without_score_or_pv() {
        assert!(
            parse_info_line("info string NNUE evaluation using nn.nnue", Side::White).is_none()
        );
        assert!(
            parse_info_line("info depth 3 currmove e2e4 currmovenumber 1", Side::White).is_none()
        );
        assert!(parse_info_line("bestmove e2e4 ponder e7e5", Side::White).is_none());
        assert!(parse_info_line("", Side::White).is_none());
    }

    #[test]
    fn ignores_bound_scores() {
        let line = "info depth 10 multipv 1 score cp 80 lowerbound nodes 5 pv e2e4";
        assert!(parse_info_line(line, Side::White).is_none());
        let line = "info depth 10 multipv 1 score cp 80 upperbound nodes 5 pv e2e4";
        assert!(parse_info_line(line, Side::White).is_none());
    }

    #[test]
    fn side_to_move_is_read_from_the_fen() {
        assert_eq!(
            side_to_move("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
            Side::White
        );
        assert_eq!(
            side_to_move("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"),
            Side::Black
        );
    }

    #[test]
    fn missing_binary_is_reported() {
        let config = EngineConfig::new(PathBuf::from("definitely/not/here/stockfish"));
        assert!(matches!(
            UciEngine::start(config),
            Err(EngineError::NotFound)
        ));
    }

    #[test]
    fn explicit_missing_path_is_not_located() {
        assert!(locate_stockfish(Some(Path::new("definitely/not/here"))).is_none());
    }

    #[test]
    fn scripted_analyzer_returns_responses_in_order_then_errors() {
        let line = AnalysisLine {
            rank: 1,
            eval: Eval::Cp(0),
            depth: 1,
            pv: vec!["e2e4".into()],
        };
        let mut a = ScriptedAnalyzer::new(vec![PositionAnalysis {
            lines: vec![line.clone()],
        }]);
        assert_eq!(a.analyze("fen", &Limits::default()).unwrap().lines[0], line);
        assert!(a.analyze("fen", &Limits::default()).is_err());
        assert_eq!(a.calls, 2);
    }

    #[test]
    fn the_engines_directory_is_found_from_a_nested_directory_but_not_from_elsewhere() {
        let root =
            std::env::temp_dir().join(format!("chess-analyzer-locate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let nested = root.join("project").join("target").join("debug");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join("project").join("engines")).unwrap();
        let name = engine_file_name();
        let engine = root.join("project").join("engines").join(name);
        std::fs::write(&engine, b"").unwrap();
        let elsewhere = root.join("other");
        std::fs::create_dir_all(&elsewhere).unwrap();

        assert_eq!(find_in_engines_dir(&nested, name), Some(engine));
        assert_eq!(find_in_engines_dir(&elsewhere, name), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn live_line(rank: u32, depth: u32, first_move: &str) -> AnalysisLine {
        AnalysisLine {
            rank,
            eval: Eval::Cp(rank as i32 * 10),
            depth,
            pv: vec![first_move.to_string()],
        }
    }

    fn at_depth(depth: u32, first_move: &str) -> PositionAnalysis {
        PositionAnalysis {
            lines: vec![live_line(1, depth, first_move)],
        }
    }

    /// Everything the engine reports right now.
    fn drain(engine: &mut ScriptedLiveEngine) -> Vec<SearchUpdate> {
        let mut updates = Vec::new();
        while let Some(update) = engine.poll(Duration::ZERO).unwrap() {
            updates.push(update);
        }
        updates
    }

    #[test]
    fn a_bounded_scripted_search_stops_at_its_depth_and_finishes() {
        let mut engine = ScriptedLiveEngine::new(|_| {
            vec![
                at_depth(4, "e2e4"),
                at_depth(8, "e2e4"),
                at_depth(12, "e2e4"),
            ]
        });
        engine.start("fen", 3, SearchLimit::Depth(8)).unwrap();
        let depths: Vec<Option<u32>> = drain(&mut engine)
            .iter()
            .map(|u| match u {
                SearchUpdate::Depth(a) => Some(a.lines[0].depth),
                SearchUpdate::Finished => None,
            })
            .collect();
        assert_eq!(depths, [Some(4), Some(8), None]);
    }

    #[test]
    fn an_infinite_scripted_search_never_finishes() {
        let mut engine =
            ScriptedLiveEngine::new(|_| vec![at_depth(4, "e2e4"), at_depth(8, "e2e4")]);
        engine.start("fen", 3, SearchLimit::Infinite).unwrap();
        let updates = drain(&mut engine);
        assert_eq!(updates.len(), 2);
        assert!(!updates.contains(&SearchUpdate::Finished));
        assert_eq!(engine.poll(Duration::ZERO).unwrap(), None);
    }

    #[test]
    fn starting_a_search_discards_what_the_previous_one_had_not_delivered() {
        let mut engine = ScriptedLiveEngine::new(|fen| vec![at_depth(6, fen)]);
        engine.start("first", 1, SearchLimit::Infinite).unwrap();
        engine.start("second", 1, SearchLimit::Infinite).unwrap();
        match drain(&mut engine).as_slice() {
            [SearchUpdate::Depth(a)] => assert_eq!(a.lines[0].pv, ["second"]),
            other => panic!("unexpected updates {other:?}"),
        }
        let log = engine.log();
        let starts = &log.lock().unwrap().starts;
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[1], ("second".to_string(), 1, SearchLimit::Infinite));
    }

    #[test]
    fn a_scripted_engine_can_be_told_to_fail() {
        let mut engine = ScriptedLiveEngine::new(|_| vec![at_depth(6, "e2e4")]);
        engine.log().lock().unwrap().fail_polls = 1;
        assert!(matches!(
            engine.poll(Duration::ZERO),
            Err(EngineError::Io(_))
        ));
        assert!(engine.poll(Duration::ZERO).is_ok(), "only one poll fails");

        engine.log().lock().unwrap().fail_recover = true;
        assert!(engine.recover().is_err());
        assert_eq!(engine.log().lock().unwrap().recoveries, 1);
    }

    #[test]
    fn live_engines_can_be_used_as_trait_objects_on_another_thread() {
        let engine: Box<dyn LiveEngine + Send> = Box::new(ScriptedLiveEngine::new(|_| Vec::new()));
        std::thread::spawn(move || drop(engine)).join().unwrap();
    }
}
