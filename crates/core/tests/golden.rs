//! Golden review tests.
//!
//! Each fixture game in `data/fixtures/` has recorded engine analyses (`<name>.analysis.json`)
//! and an expected review summary (`<name>.golden.txt`). The review is replayed from the
//! recorded analyses, so these tests need no Stockfish and do not depend on its version; they
//! show exactly what changes when classification heuristics or thresholds are tuned.
//!
//! To accept an intended change:  UPDATE_GOLDEN=1 cargo test -p chess-analyzer-core --test golden
//! To re-record analyses:         cargo test -p chess-analyzer-core --test golden -- --ignored record

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use chess_analyzer_core::engine::{
    Analyzer, EngineConfig, EngineError, Limits, PositionAnalysis, ScriptedAnalyzer, UciEngine,
    locate_stockfish,
};
use chess_analyzer_core::game::parse_pgn;
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::review::{Review, ReviewOptions, review_game};

const FIXTURES: &[&str] = &["fools_mate", "opera_game"];

fn fixture(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/fixtures")
        .join(file)
}

/// A stable, human-readable summary. Floats are rounded so tiny platform differences in
/// `exp()` cannot change it.
fn snapshot(review: &Review) -> String {
    let header = |key: &str| review.headers.get(key).cloned().unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!(
        "{} vs {} ({})\n",
        header("White"),
        header("Black"),
        header("Result")
    ));
    match &review.opening {
        Some(o) => out.push_str(&format!("opening: {} {}\n", o.eco, o.name)),
        None => out.push_str("opening: none\n"),
    }
    let acc = |v: Option<f64>| v.map_or("n/a".to_string(), |v| format!("{v:.1}"));
    out.push_str(&format!(
        "accuracy: white={} black={}\n",
        acc(review.accuracy.white),
        acc(review.accuracy.black)
    ));
    out.push_str(&format!("critical plies: {:?}\n", review.critical_plies));
    for m in &review.moves {
        out.push_str(&format!(
            "{:>3} {:<8} {:<10} loss={:>5.1} best={}\n",
            m.ply,
            m.san,
            format!("{:?}", m.class),
            m.loss,
            m.best_san.as_deref().unwrap_or("-")
        ));
    }
    out
}

fn replay(name: &str) -> Review {
    let pgn = std::fs::read_to_string(fixture(&format!("{name}.pgn"))).expect("fixture PGN");
    let recorded = std::fs::read_to_string(fixture(&format!("{name}.analysis.json")))
        .expect("recorded analyses; see the module docs to record them");
    let analyses: Vec<PositionAnalysis> = serde_json::from_str(&recorded).expect("valid analyses");
    let game = parse_pgn(&pgn).expect("valid PGN").remove(0);
    let mut analyzer = ScriptedAnalyzer::new(analyses);
    review_game(
        &game,
        &mut analyzer,
        &ReviewOptions::default(),
        OpeningBook::bundled(),
        &AtomicBool::new(false),
        |_| {},
    )
    .expect("review replays")
}

#[test]
fn reviews_match_their_golden_snapshots() {
    for name in FIXTURES {
        let actual = snapshot(&replay(name));
        let golden_path = fixture(&format!("{name}.golden.txt"));
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(&golden_path, &actual).expect("write golden");
            continue;
        }
        let expected = std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|_| panic!("missing {golden_path:?}; run with UPDATE_GOLDEN=1"));
        assert_eq!(
            actual.replace("\r\n", "\n"),
            expected.replace("\r\n", "\n"),
            "review of {name} changed; if intended, re-run with UPDATE_GOLDEN=1"
        );
    }
}

/// Wraps an analyzer and remembers every answer, in call order.
struct Recording<A: Analyzer> {
    inner: A,
    recorded: Vec<PositionAnalysis>,
}

impl<A: Analyzer> Analyzer for Recording<A> {
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        let analysis = self.inner.analyze(fen, limits)?;
        self.recorded.push(analysis.clone());
        Ok(analysis)
    }

    fn engine_id(&self) -> String {
        self.inner.engine_id()
    }
}

#[test]
#[ignore = "needs Stockfish; re-records data/fixtures/*.analysis.json"]
fn record() {
    let path = locate_stockfish(None).expect("Stockfish (run scripts/setup-stockfish)");
    let engine = UciEngine::start(EngineConfig::new(path)).expect("engine starts");
    let mut recording = Recording {
        inner: engine,
        recorded: Vec::new(),
    };
    for name in FIXTURES {
        recording.recorded.clear();
        let pgn = std::fs::read_to_string(fixture(&format!("{name}.pgn"))).expect("fixture PGN");
        let game = parse_pgn(&pgn).expect("valid PGN").remove(0);
        let options = ReviewOptions {
            limits: Limits {
                depth: 14,
                multipv: 3,
            },
            ..ReviewOptions::default()
        };
        review_game(
            &game,
            &mut recording,
            &options,
            OpeningBook::bundled(),
            &AtomicBool::new(false),
            |_| {},
        )
        .expect("review");
        let json = serde_json::to_string_pretty(&recording.recorded).expect("serialize");
        std::fs::write(fixture(&format!("{name}.analysis.json")), json).expect("write analyses");
    }
}
