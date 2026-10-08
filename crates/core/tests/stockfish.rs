//! Integration tests against a real Stockfish. They skip (and say so) if no binary is found:
//! run `scripts/setup-stockfish` or set STOCKFISH_PATH.

use std::sync::atomic::AtomicBool;

use chess_analyzer_core::cache::CachedAnalyzer;
use chess_analyzer_core::classify::MoveClass;
use chess_analyzer_core::engine::{Analyzer, EngineConfig, Limits, UciEngine, locate_stockfish};
use chess_analyzer_core::eval::Eval;
use chess_analyzer_core::game::parse_pgn;
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::review::{ReviewOptions, review_game};

fn engine() -> Option<UciEngine> {
    let Some(path) = locate_stockfish(None) else {
        eprintln!(
            "SKIPPED: Stockfish not found (run scripts/setup-stockfish or set STOCKFISH_PATH)"
        );
        return None;
    };
    Some(UciEngine::start(EngineConfig::new(path)).expect("Stockfish starts"))
}

const SHALLOW: Limits = Limits {
    depth: 8,
    multipv: 3,
};

#[test]
fn reports_its_name() {
    let Some(engine) = engine() else { return };
    assert!(
        engine.engine_id().contains("Stockfish"),
        "{}",
        engine.engine_id()
    );
}

#[test]
fn start_position_gives_ranked_lines_and_a_roughly_equal_score() {
    let Some(mut engine) = engine() else { return };
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let analysis = engine.analyze(fen, &SHALLOW).unwrap();
    assert_eq!(analysis.lines.len(), 3);
    assert_eq!(
        analysis.lines.iter().map(|l| l.rank).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(analysis.lines.iter().all(|l| !l.pv.is_empty()));
    match analysis.lines[0].eval {
        Eval::Cp(cp) => assert!(
            cp.abs() < 100,
            "start position should be near equal, got {cp}"
        ),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn white_mate_in_one_is_reported_as_positive_mate() {
    let Some(mut engine) = engine() else { return };
    let analysis = engine
        .analyze("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", &SHALLOW)
        .unwrap();
    assert_eq!(analysis.lines[0].eval, Eval::Mate(1));
    assert_eq!(analysis.lines[0].pv[0], "a1a8");
}

#[test]
fn black_mate_in_one_is_reported_as_negative_mate() {
    let Some(mut engine) = engine() else { return };
    let analysis = engine
        .analyze("r5k1/8/8/8/8/8/5PPP/6K1 b - - 0 1", &SHALLOW)
        .unwrap();
    assert_eq!(analysis.lines[0].eval, Eval::Mate(-1));
    assert_eq!(analysis.lines[0].pv[0], "a8a1");
}

#[test]
fn engine_survives_many_positions_in_a_row() {
    let Some(mut engine) = engine() else { return };
    let game = parse_pgn("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 *")
        .unwrap()
        .remove(0);
    for fen in &game.positions {
        engine.analyze(fen, &SHALLOW).unwrap();
    }
}

#[test]
fn a_full_review_flags_the_blunder_in_the_fools_mate() {
    let Some(mut engine) = engine() else { return };
    let game = parse_pgn("1. f3 e5 2. g4 Qh4# 0-1").unwrap().remove(0);
    let review = review_game(
        &game,
        &mut engine,
        &ReviewOptions {
            limits: SHALLOW,
            ..ReviewOptions::default()
        },
        &OpeningBook::empty(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(review.moves[2].san, "g4");
    assert_eq!(review.moves[2].class, MoveClass::Blunder);
    assert_eq!(review.moves[3].class, MoveClass::Best);
    assert_eq!(
        review.evals[4],
        Eval::Checkmate(chess_analyzer_core::eval::Side::Black)
    );
}

#[test]
fn a_cached_second_review_never_touches_the_engine() {
    let Some(engine) = engine() else { return };
    let game = parse_pgn("1. e4 e5 2. Nf3 Nc6 *").unwrap().remove(0);
    let options = ReviewOptions {
        limits: SHALLOW,
        ..ReviewOptions::default()
    };
    let mut cached = CachedAnalyzer::in_memory(engine).unwrap();
    let first = review_game(
        &game,
        &mut cached,
        &options,
        &OpeningBook::empty(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(cached.misses, 5);
    let second = review_game(
        &game,
        &mut cached,
        &options,
        &OpeningBook::empty(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(cached.hits, 5);
    assert_eq!(first, second);
}

#[test]
fn a_position_with_one_legal_move_returns_one_line_despite_multipv() {
    let Some(mut engine) = engine() else { return };
    // Black's king on a8 is in check from the rook; Kb8 is the only legal move.
    let analysis = engine
        .analyze("k7/8/1K6/8/8/8/8/R7 b - - 0 1", &SHALLOW)
        .unwrap();
    assert_eq!(analysis.lines.len(), 1);
    assert_eq!(analysis.lines[0].pv[0], "a8b8");
}

#[test]
fn a_blunder_inside_a_named_opening_line_is_still_flagged() {
    let Some(mut engine) = engine() else { return };
    // "Fool's Mate" is a named line in the bundled opening data.
    let game = parse_pgn("1. f3 e5 2. g4 Qh4# 0-1").unwrap().remove(0);
    let review = review_game(
        &game,
        &mut engine,
        &ReviewOptions {
            limits: SHALLOW,
            ..ReviewOptions::default()
        },
        OpeningBook::bundled(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert!(
        review
            .opening
            .as_ref()
            .unwrap()
            .name
            .contains("Fool's Mate")
    );
    assert_eq!(review.moves[2].class, MoveClass::Blunder);
    assert!(review.critical_plies.contains(&3));
}

#[test]
fn a_timeout_restarts_the_engine_and_surfaces_an_error() {
    use chess_analyzer_core::engine::EngineError;
    use std::time::Duration;

    let Some(path) = locate_stockfish(None) else {
        eprintln!("SKIPPED: Stockfish not found");
        return;
    };
    let mut config = EngineConfig::new(path);
    config.timeout = Duration::from_millis(500);
    let mut engine = UciEngine::start(config).expect("Stockfish starts");
    let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    let err = engine
        .analyze(
            start,
            &Limits {
                depth: 60,
                multipv: 3,
            },
        )
        .unwrap_err();
    assert!(matches!(err, EngineError::Timeout(_)), "{err:?}");

    // The hung process was replaced, so the engine still answers quick requests.
    let quick = engine
        .analyze(
            start,
            &Limits {
                depth: 4,
                multipv: 1,
            },
        )
        .unwrap();
    assert_eq!(quick.lines.len(), 1);
}
