//! Stockfish integration over UCI.
//!
//! `Analyzer` is the seam the rest of the crate depends on. `UciEngine` is the real
//! implementation; `ScriptedAnalyzer` is a canned one for tests.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
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
}
