//! Integration tests against a real Stockfish. They skip (and say so) if no binary is found:
//! run `scripts/setup-stockfish` or set STOCKFISH_PATH.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chess_analyzer_core::cache::CachedAnalyzer;
use chess_analyzer_core::classify::{MoveClass, Thresholds};
use chess_analyzer_core::engine::{
    Analyzer, EngineConfig, Limits, LiveEngine, SearchLimit, SearchUpdate, UciEngine,
    locate_stockfish,
};
use chess_analyzer_core::eval::Eval;
use chess_analyzer_core::game::parse_pgn;
use chess_analyzer_core::live::{LiveConfig, LiveEvent, LiveSession, LiveSink};
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::review::{ReviewOptions, review_game};
use shakmaty::fen::Fen;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess};

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

// ---- live (interruptible) searches

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
const AFTER_E4: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";

/// Polls until `done` accepts an update, returning every update seen. Gives up after 20 s.
fn poll_until(engine: &mut UciEngine, done: impl Fn(&SearchUpdate) -> bool) -> Vec<SearchUpdate> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut updates = Vec::new();
    while Instant::now() < deadline {
        if let Some(update) = engine.poll(Duration::from_millis(100)).unwrap() {
            let last = done(&update);
            updates.push(update);
            if last {
                return updates;
            }
        }
    }
    panic!("the search did not get there in time; saw {updates:?}");
}

fn depth_of(update: &SearchUpdate) -> Option<u32> {
    match update {
        SearchUpdate::Depth(analysis) => Some(analysis.lines[0].depth),
        SearchUpdate::Finished => None,
    }
}

fn is_legal(fen: &str, uci: &str) -> bool {
    let position: Chess = fen
        .parse::<Fen>()
        .unwrap()
        .into_position(CastlingMode::Standard)
        .unwrap();
    UciMove::from_ascii(uci.as_bytes())
        .ok()
        .and_then(|mv| mv.to_move(&position).ok())
        .is_some()
}

#[test]
fn an_infinite_search_deepens_with_all_lines_at_each_depth_until_stopped() {
    let Some(mut engine) = engine() else { return };
    engine.start(START, 3, SearchLimit::Infinite).unwrap();
    let updates = poll_until(&mut engine, |u| depth_of(u).is_some_and(|d| d >= 10));

    let mut previous = 0;
    for update in &updates {
        let SearchUpdate::Depth(analysis) = update else {
            panic!("an infinite search must not finish: {update:?}");
        };
        let depth = analysis.lines[0].depth;
        assert!(
            depth > previous,
            "depths must increase: {previous} then {depth}"
        );
        previous = depth;
        assert_eq!(analysis.lines.len(), 3, "depth {depth}");
        assert_eq!(
            analysis.lines.iter().map(|l| l.rank).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(analysis.lines.iter().all(|l| l.depth == depth));
    }

    engine.stop().unwrap();
    assert_eq!(engine.poll(Duration::from_millis(50)).unwrap(), None);
}

#[test]
fn a_depth_limited_search_reports_up_to_its_depth_then_finishes() {
    let Some(mut engine) = engine() else { return };
    engine.start(START, 2, SearchLimit::Depth(6)).unwrap();
    let updates = poll_until(&mut engine, |u| *u == SearchUpdate::Finished);
    let depths: Vec<u32> = updates.iter().filter_map(depth_of).collect();
    assert_eq!(depths.last(), Some(&6));
    assert!(
        depths.windows(2).all(|pair| pair[0] < pair[1]),
        "{depths:?}"
    );
    assert_eq!(engine.poll(Duration::from_millis(50)).unwrap(), None);
}

#[test]
fn a_new_search_never_sees_the_answer_of_the_one_it_replaced() {
    let Some(mut engine) = engine() else { return };
    engine.start(START, 1, SearchLimit::Infinite).unwrap();
    poll_until(&mut engine, |u| depth_of(u).is_some_and(|d| d >= 4));

    // Black is to move here, so every line must begin with a legal move for Black; a leftover
    // line from the first search would begin with a White move.
    engine.start(AFTER_E4, 2, SearchLimit::Depth(6)).unwrap();
    let updates = poll_until(&mut engine, |u| *u == SearchUpdate::Finished);
    for update in &updates {
        if let SearchUpdate::Depth(analysis) = update {
            for line in &analysis.lines {
                assert!(is_legal(AFTER_E4, &line.pv[0]), "{:?}", line.pv);
            }
        }
    }
    assert_eq!(updates.last(), Some(&SearchUpdate::Finished));
}

#[test]
fn restarting_again_and_again_stays_in_step() {
    let Some(mut engine) = engine() else { return };
    for round in 0..30 {
        let (fen, black) = if round % 2 == 0 {
            (START, false)
        } else {
            (AFTER_E4, true)
        };
        engine.start(fen, 1, SearchLimit::Infinite).unwrap();
        // Sometimes interrupt at once, sometimes after a first answer.
        if round % 3 == 0 {
            poll_until(&mut engine, |u| depth_of(u).is_some());
        }
        engine.start(fen, 1, SearchLimit::Depth(3)).unwrap();
        let updates = poll_until(&mut engine, |u| *u == SearchUpdate::Finished);
        for update in &updates {
            if let SearchUpdate::Depth(analysis) = update {
                assert!(
                    is_legal(fen, &analysis.lines[0].pv[0]),
                    "round {round} (black to move: {black})"
                );
            }
        }
    }
}

#[test]
fn stopping_an_idle_engine_is_harmless_and_it_still_analyses_afterwards() {
    let Some(mut engine) = engine() else { return };
    engine.stop().unwrap();
    engine.start(START, 1, SearchLimit::Infinite).unwrap();
    poll_until(&mut engine, |u| depth_of(u).is_some());
    engine.stop().unwrap();
    engine.stop().unwrap();

    let analysis = engine.analyze(START, &SHALLOW).unwrap();
    assert_eq!(analysis.lines.len(), 3);
}

#[test]
fn a_recovered_engine_searches_again() {
    let Some(mut engine) = engine() else { return };
    engine.start(START, 1, SearchLimit::Infinite).unwrap();
    poll_until(&mut engine, |u| depth_of(u).is_some());
    engine.recover().unwrap();
    assert_eq!(engine.poll(Duration::from_millis(50)).unwrap(), None);

    engine.start(START, 1, SearchLimit::Depth(4)).unwrap();
    poll_until(&mut engine, |u| *u == SearchUpdate::Finished);
}

// ---- a live session on the real engine

#[derive(Default)]
struct Collect(Mutex<Vec<LiveEvent>>);

impl LiveSink for Collect {
    fn emit(&self, event: LiveEvent) {
        self.0.lock().unwrap().push(event);
    }
}

impl Collect {
    fn wait_for(&self, what: &str, done: impl Fn(&[LiveEvent]) -> bool) -> Vec<LiveEvent> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let events = self.0.lock().unwrap().clone();
            if done(&events) {
                return events;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {events:#?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn session(sink: &Arc<Collect>) -> Option<LiveSession> {
    let engine = engine()?;
    let config = LiveConfig {
        multipv: 2,
        thresholds: Thresholds::default(),
        book: OpeningBook::bundled(),
    };
    Some(LiveSession::start(engine, sink.clone(), config))
}

fn moves(list: &[&str]) -> Vec<String> {
    list.iter().map(|m| m.to_string()).collect()
}

fn classified(events: &[LiveEvent], ply: usize) -> bool {
    events
        .iter()
        .any(|e| matches!(e, LiveEvent::Move { review, .. } if review.ply == ply))
}

fn reached(events: &[LiveEvent], revision: u64, index: usize, depth: u32) -> bool {
    events.iter().any(|e| {
        matches!(e, LiveEvent::Position { revision: r, index: i, depth: d, .. }
            if *r == revision && *i == index && *d >= depth)
    })
}

#[test]
fn a_live_session_flags_a_blunder_as_it_is_entered() {
    let sink = Arc::new(Collect::default());
    let Some(session) = session(&sink) else {
        return;
    };
    session.set_moves(1, moves(&["f2f3", "e7e5", "g2g4"]));
    let events = sink.wait_for("g4 classified and a deep look after it", |e| {
        classified(e, 3) && reached(e, 1, 3, 12)
    });

    let blunder = events.iter().rev().find_map(|e| match e {
        LiveEvent::Move { review, .. } if review.ply == 3 => Some(review.clone()),
        _ => None,
    });
    let blunder = blunder.expect("g4 is classified");
    assert_eq!(blunder.san, "g4");
    assert_eq!(blunder.class, MoveClass::Blunder);

    // The lines for the newest position come with their moves in SAN: Black mates with Qh4.
    let lines = events.iter().rev().find_map(|e| match e {
        LiveEvent::Position {
            index: 3, lines, ..
        } => Some(lines.clone()),
        _ => None,
    });
    let best = &lines.unwrap()[0];
    assert_eq!(best.eval, Eval::Mate(-1));
    assert_eq!(best.pv_san[0], "Qh4#");
}

#[test]
fn a_live_session_keeps_up_with_moves_entered_in_quick_succession() {
    let sink = Arc::new(Collect::default());
    let Some(session) = session(&sink) else {
        return;
    };
    let game = [
        "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6",
    ];
    for (revision, count) in (1..=game.len()).enumerate() {
        session.set_moves(revision as u64 + 1, moves(&game[..count]));
    }
    let last = game.len() as u64;
    let events = sink.wait_for("every move classified and a deep final look", |e| {
        (1..=game.len()).all(|ply| classified(e, ply)) && reached(e, last, game.len(), 12)
    });

    // Every move of the final list was classified under the final revision, or an earlier one
    // that is still true of it, and nothing arrived from a revision that was never sent.
    for ply in 1..=game.len() {
        assert!(
            events.iter().any(|e| matches!(
                e,
                LiveEvent::Move { review, .. } if review.ply == ply
            )),
            "ply {ply} was never classified"
        );
    }
    assert!(events.iter().all(|e| match e {
        LiveEvent::Position { revision, .. }
        | LiveEvent::Move { revision, .. }
        | LiveEvent::Error { revision, .. } => (1..=last).contains(revision),
    }));
    assert!(
        events.iter().all(|e| !matches!(e, LiveEvent::Error { .. })),
        "no errors expected: {events:#?}"
    );
}
