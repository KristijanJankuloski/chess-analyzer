//! The review pipeline: a `Game` plus an `Analyzer` becomes a `Review`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, Color, Position, Role};
use thiserror::Error;
use ts_rs::TS;

use crate::classify::{MoveClass, MoveContext, Thresholds, classify};
use crate::engine::{AnalysisLine, Analyzer, EngineError, Limits, PositionAnalysis};
use crate::eval::{Eval, Side, move_accuracy};
use crate::game::Game;
use crate::openings::{Opening, OpeningBook};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReviewError {
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("the review was cancelled")]
    Cancelled,
    #[error("invalid review options: {0}")]
    InvalidOptions(String),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReviewOptions {
    pub limits: Limits,
    pub thresholds: Thresholds,
}

/// `done` of `total` positions analysed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MoveReview {
    /// 1-based ply number.
    pub ply: usize,
    /// The move number as written in PGN (1 for White's and Black's first moves).
    pub move_number: u32,
    pub side: Side,
    pub san: String,
    pub uci: String,
    pub class: MoveClass,
    /// Evaluation of the position before the move (engine's best line), White's point of view.
    pub eval_before: Eval,
    /// Evaluation after the played move, White's point of view.
    pub eval_after: Eval,
    pub best_uci: Option<String>,
    pub best_san: Option<String>,
    /// The engine's principal variation from the position before the move, in UCI.
    pub best_pv: Vec<String>,
    /// Win-percentage points lost against the best move (0 for the best move).
    pub loss: f64,
    /// 0 to 100.
    pub accuracy: f64,
    pub critical: bool,
    /// A few plain sentences about the move, written from the engine's own lines (see
    /// `commentary`). `None` for a review saved before commentary existed, or when the move's
    /// position could not be read.
    #[serde(default)]
    pub commentary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Accuracy {
    pub white: Option<f64>,
    pub black: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Review {
    pub headers: BTreeMap<String, String>,
    pub opening: Option<Opening>,
    pub engine: String,
    pub limits: Limits,
    /// Evaluation of every position (White's point of view); index 0 is the start,
    /// so `evals.len() == moves.len() + 1`.
    pub evals: Vec<Eval>,
    pub moves: Vec<MoveReview>,
    pub accuracy: Accuracy,
    /// 1-based plies worth a look: inaccuracies, mistakes, misses, blunders, brilliant and great moves.
    pub critical_plies: Vec<usize>,
}

fn material(pos: &Chess, color: Color) -> i32 {
    let board = pos.board();
    let mine = board.by_color(color);
    let count = |role: Role| (board.by_role(role) & mine).count() as i32;
    count(Role::Pawn)
        + 3 * count(Role::Knight)
        + 3 * count(Role::Bishop)
        + 5 * count(Role::Rook)
        + 9 * count(Role::Queen)
}

pub(crate) fn balance(pos: &Chess, color: Color) -> i32 {
    material(pos, color) - material(pos, color.other())
}

pub(crate) fn apply_uci(pos: &Chess, uci: &str) -> Option<Chess> {
    let mv = UciMove::from_ascii(uci.as_bytes())
        .ok()?
        .to_move(pos)
        .ok()?;
    let mut next = pos.clone();
    next.play_unchecked(mv);
    Some(next)
}

pub(crate) fn uci_to_san(pos: &Chess, uci: &str) -> Option<String> {
    let mv = UciMove::from_ascii(uci.as_bytes())
        .ok()?
        .to_move(pos)
        .ok()?;
    Some(SanPlus::from_move(pos.clone(), mv).to_string())
}

/// What the engine would say about a finished game, without asking it.
pub(crate) fn terminal_analysis(pos: &Chess) -> Option<PositionAnalysis> {
    let eval = if pos.is_checkmate() {
        Eval::Checkmate(Side::from(pos.turn().other()))
    } else if pos.is_stalemate() || pos.is_insufficient_material() {
        Eval::Cp(0)
    } else {
        return None;
    };
    Some(PositionAnalysis {
        lines: vec![AnalysisLine {
            rank: 1,
            eval,
            depth: 0,
            pv: Vec::new(),
        }],
    })
}

/// What `review_game_streaming` reports while it works.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewEvent {
    /// Position `index` (0 = the start) has been analysed; `eval` is its best line.
    Analysed {
        index: usize,
        total: usize,
        eval: Eval,
    },
    /// Move `ply` is classified. Sent as soon as the position after it has been analysed.
    Move(MoveReview),
}

/// The unbroken run of plies whose resulting position is a known opening position,
/// and the deepest named opening along it.
pub(crate) fn book_prefix(game: &Game, book: &OpeningBook) -> (usize, Option<Opening>) {
    let mut book_plies = 0;
    let mut opening = None;
    for ply in 1..=game.moves.len() {
        if !book.is_book(&game.positions[ply]) {
            break;
        }
        book_plies = ply;
        if let Some(named) = book.name_of(&game.positions[ply]) {
            opening = Some(named.clone());
        }
    }
    (book_plies, opening)
}

/// Classifies move `i` (0-based) from the analyses of the position before it and the position
/// after it.
pub(crate) fn review_move(
    i: usize,
    game: &Game,
    before_analysis: &PositionAnalysis,
    after_analysis: &PositionAnalysis,
    book_plies: usize,
    prev_opponent_class: Option<MoveClass>,
    thresholds: &Thresholds,
) -> MoveReview {
    let before = game.position(i);
    let after = game.position(i + 1);
    let mover = Side::from(before.turn());
    let played = &game.moves[i];
    let lines = &before_analysis.lines;
    let best = &lines[0];

    // A move that ends the game is scored by the game's result, not by the engine's
    // "mate in 1" for the position before it; otherwise prefer the engine line for the
    // played move (same search as `best`), falling back to the next position's best line.
    let next_best = after_analysis.lines[0].eval;
    let eval_after = if terminal_analysis(&after).is_some() {
        next_best
    } else {
        lines
            .iter()
            .find(|l| l.pv.first() == Some(&played.uci))
            .map_or(next_best, |l| l.eval)
    };

    let material_swing = {
        let start = balance(&before, before.turn());
        let reply = after_analysis.lines[0].pv.first();
        let settled = reply
            .and_then(|r| apply_uci(&after, r))
            .unwrap_or_else(|| after.clone());
        balance(&settled, before.turn()) - start
    };

    let ctx = MoveContext {
        mover,
        played_uci: played.uci.clone(),
        best_uci: best.pv.first().cloned(),
        win_before: best.eval.win_percent_for(mover),
        win_second: lines.get(1).map(|l| l.eval.win_percent_for(mover)),
        win_after: eval_after.win_percent_for(mover),
        eval_before: best.eval,
        eval_after,
        in_book: i < book_plies,
        prev_opponent_class,
        material_swing,
    };
    let class = classify(&ctx, thresholds);
    let loss = ctx.loss();

    MoveReview {
        ply: i + 1,
        move_number: game.positions[i]
            .split(' ')
            .nth(5)
            .and_then(|n| n.parse().ok())
            .unwrap_or(1),
        side: mover,
        san: played.san.clone(),
        uci: played.uci.clone(),
        class,
        eval_before: best.eval,
        eval_after,
        best_uci: ctx.best_uci.clone(),
        best_san: ctx.best_uci.as_deref().and_then(|u| uci_to_san(&before, u)),
        best_pv: best.pv.clone(),
        loss,
        accuracy: move_accuracy(ctx.win_before, ctx.win_before - loss),
        critical: class.is_critical(),
        commentary: None,
    }
}

/// Like `review_game`, but reports each analysed position and each classified move as it
/// happens, so a UI can fill in while the engine is still working.
pub fn review_game_streaming(
    game: &Game,
    analyzer: &mut dyn Analyzer,
    options: &ReviewOptions,
    book: &OpeningBook,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(ReviewEvent),
) -> Result<Review, ReviewError> {
    // Stockfish treats `go depth 0` as unbounded, which would spin until the timeout.
    if options.limits.depth == 0 || options.limits.multipv == 0 {
        return Err(ReviewError::InvalidOptions(
            "depth and MultiPV must each be at least 1".to_string(),
        ));
    }
    let n = game.moves.len();
    let total = n + 1;
    let (book_plies, opening) = book_prefix(game, book);

    let mut analyses: Vec<PositionAnalysis> = Vec::with_capacity(total);
    let mut moves: Vec<MoveReview> = Vec::with_capacity(n);
    for index in 0..total {
        if cancel.load(Ordering::Relaxed) {
            return Err(ReviewError::Cancelled);
        }
        let pos = game.position(index);
        let analysis = match terminal_analysis(&pos) {
            Some(terminal) => terminal,
            None => analyzer.analyze(&game.positions[index], &options.limits)?,
        };
        let eval = analysis.lines[0].eval;
        analyses.push(analysis);
        on_event(ReviewEvent::Analysed { index, total, eval });

        if index >= 1 {
            let reviewed = review_move(
                index - 1,
                game,
                &analyses[index - 1],
                &analyses[index],
                book_plies,
                moves.last().map(|m: &MoveReview| m.class),
                &options.thresholds,
            );
            on_event(ReviewEvent::Move(reviewed.clone()));
            moves.push(reviewed);
        }
    }

    let average = |side: Side| {
        let scores: Vec<f64> = moves
            .iter()
            .filter(|m| m.side == side)
            .map(|m| m.accuracy)
            .collect();
        (!scores.is_empty()).then(|| scores.iter().sum::<f64>() / scores.len() as f64)
    };

    Ok(Review {
        headers: game.headers.clone(),
        opening,
        engine: analyzer.engine_id(),
        limits: options.limits,
        evals: analyses.iter().map(|a| a.lines[0].eval).collect(),
        accuracy: Accuracy {
            white: average(Side::White),
            black: average(Side::Black),
        },
        critical_plies: moves.iter().filter(|m| m.critical).map(|m| m.ply).collect(),
        moves,
    })
}

pub fn review_game(
    game: &Game,
    analyzer: &mut dyn Analyzer,
    options: &ReviewOptions,
    book: &OpeningBook,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<Review, ReviewError> {
    review_game_streaming(game, analyzer, options, book, cancel, |event| {
        if let ReviewEvent::Analysed { index, total, .. } = event {
            on_progress(Progress {
                done: index + 1,
                total,
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ScriptedAnalyzer;
    use crate::game::parse_pgn;

    fn line(rank: u32, eval: Eval, first_move: &str) -> AnalysisLine {
        AnalysisLine {
            rank,
            eval,
            depth: 20,
            pv: vec![first_move.to_string()],
        }
    }

    fn pa(lines: Vec<AnalysisLine>) -> PositionAnalysis {
        PositionAnalysis { lines }
    }

    fn flat(first_move: &str) -> PositionAnalysis {
        pa(vec![line(1, Eval::Cp(0), first_move)])
    }

    fn run(pgn: &str, script: Vec<PositionAnalysis>, book: &OpeningBook) -> Review {
        let game = parse_pgn(pgn).unwrap().remove(0);
        let mut analyzer = ScriptedAnalyzer::new(script);
        review_game(
            &game,
            &mut analyzer,
            &ReviewOptions::default(),
            book,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap()
    }

    fn fools_mate_script() -> Vec<PositionAnalysis> {
        vec![
            // 1. f3?!  (best was e4)
            pa(vec![
                line(1, Eval::Cp(20), "e2e4"),
                line(2, Eval::Cp(15), "d2d4"),
            ]),
            // 1... e5
            pa(vec![
                line(1, Eval::Cp(-60), "e7e5"),
                line(2, Eval::Cp(-70), "e7e6"),
            ]),
            // 2. g4??  (best was d4)
            pa(vec![
                line(1, Eval::Cp(-50), "d2d4"),
                line(2, Eval::Cp(-55), "e2e4"),
            ]),
            // 2... Qh4#
            pa(vec![line(1, Eval::Mate(-1), "d8h4")]),
        ]
    }

    #[test]
    fn reviews_the_fools_mate() {
        let review = run(
            "1. f3 e5 2. g4 Qh4# 0-1",
            fools_mate_script(),
            &OpeningBook::empty(),
        );

        assert_eq!(review.moves.len(), 4);
        assert_eq!(review.evals.len(), 5);
        assert_eq!(review.evals[4], Eval::Checkmate(Side::Black));

        assert_eq!(review.moves[0].class, MoveClass::Inaccuracy);
        assert_eq!(review.moves[1].class, MoveClass::Best);
        assert_eq!(review.moves[2].class, MoveClass::Blunder);
        assert_eq!(review.moves[3].class, MoveClass::Best);

        assert_eq!(review.moves[2].best_uci.as_deref(), Some("d2d4"));
        assert_eq!(review.moves[2].best_san.as_deref(), Some("d4"));
        assert_eq!(review.moves[2].eval_after, Eval::Mate(-1));
        assert_eq!(review.critical_plies, vec![1, 3]);
        assert!(review.accuracy.black.unwrap() > review.accuracy.white.unwrap());
        assert_eq!(review.engine, "scripted");
    }

    #[test]
    fn the_best_move_has_full_accuracy_and_zero_loss() {
        let review = run(
            "1. f3 e5 2. g4 Qh4# 0-1",
            fools_mate_script(),
            &OpeningBook::empty(),
        );
        assert_eq!(review.moves[1].loss, 0.0);
        assert!((review.moves[1].accuracy - 100.0).abs() < 1e-9);
        assert!(review.moves[2].accuracy < 20.0);
    }

    #[test]
    fn book_moves_and_the_opening_name_come_from_the_book() {
        let tsv = "eco\tname\tpgn\nB00\tKing's Pawn\t1. e4\nC20\tKing's Pawn Game\t1. e4 e5\n";
        let book = OpeningBook::from_tsv(tsv).unwrap();
        let script = vec![
            pa(vec![line(1, Eval::Cp(20), "e2e4")]),
            pa(vec![line(1, Eval::Cp(-20), "e7e5")]),
            pa(vec![line(1, Eval::Cp(20), "g1f3")]),
            pa(vec![line(1, Eval::Cp(-20), "b8c6")]),
        ];
        let review = run("1. e4 e5 2. Qh5 *", script, &book);
        let classes: Vec<_> = review.moves.iter().map(|m| m.class).collect();
        assert_eq!(classes, [MoveClass::Book, MoveClass::Book, MoveClass::Good]);
        assert_eq!(review.opening.as_ref().unwrap().name, "King's Pawn Game");
    }

    #[test]
    fn a_blunder_inside_a_known_line_is_still_flagged() {
        let tsv = "eco\tname\tpgn\nA00\tFool's Mate\t1. f3 e5 2. g4 Qh4#\n";
        let book = OpeningBook::from_tsv(tsv).unwrap();
        let review = run("1. f3 e5 2. g4 Qh4# 0-1", fools_mate_script(), &book);
        assert_eq!(review.opening.as_ref().unwrap().name, "Fool's Mate");
        assert_eq!(review.moves[0].class, MoveClass::Book);
        assert_eq!(review.moves[2].class, MoveClass::Blunder);
    }

    #[test]
    fn a_game_from_a_black_to_move_position_is_numbered_from_its_fen() {
        let pgn = "[FEN \"rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 7\"]\n\n7... e5 8. Nf3 *";
        let script = vec![flat("e7e5"), flat("g1f3"), flat("b8c6")];
        let review = run(pgn, script, &OpeningBook::empty());
        assert_eq!(review.moves[0].side, Side::Black);
        assert_eq!(review.moves[0].move_number, 7);
        assert_eq!(review.moves[1].side, Side::White);
        assert_eq!(review.moves[1].move_number, 8);
    }

    #[test]
    fn the_mating_move_ends_in_checkmate_not_mate_in_one() {
        let review = run(
            "1. f3 e5 2. g4 Qh4# 0-1",
            fools_mate_script(),
            &OpeningBook::empty(),
        );
        assert_eq!(review.moves[3].san, "Qh4#");
        assert_eq!(review.moves[3].eval_after, Eval::Checkmate(Side::Black));
        assert_eq!(review.moves[3].eval_after, *review.evals.last().unwrap());
    }

    #[test]
    fn zero_depth_or_multipv_is_rejected_before_analysing() {
        let game = parse_pgn("1. e4 *").unwrap().remove(0);
        for limits in [
            Limits {
                depth: 0,
                multipv: 3,
            },
            Limits {
                depth: 20,
                multipv: 0,
            },
        ] {
            let mut analyzer = ScriptedAnalyzer::new(vec![flat("e2e4"), flat("e7e5")]);
            let options = ReviewOptions {
                limits,
                ..ReviewOptions::default()
            };
            let err = review_game(
                &game,
                &mut analyzer,
                &options,
                &OpeningBook::empty(),
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap_err();
            assert!(matches!(err, ReviewError::InvalidOptions(_)), "{err:?}");
            assert_eq!(analyzer.calls, 0);
        }
    }
    #[test]
    fn a_sound_sacrifice_is_brilliant() {
        let script = vec![
            flat("e2e4"),
            flat("e7e5"),
            flat("f1c4"),
            flat("g8f6"),
            pa(vec![
                line(1, Eval::Cp(60), "c4f7"),
                line(2, Eval::Cp(10), "d2d3"),
            ]),
            pa(vec![line(1, Eval::Cp(55), "e8f7")]),
            flat("d2d3"),
        ];
        let review = run(
            "1. e4 e5 2. Bc4 Nf6 3. Bxf7+ Kxf7 *",
            script,
            &OpeningBook::empty(),
        );
        assert_eq!(review.moves[4].san, "Bxf7+");
        assert_eq!(review.moves[4].class, MoveClass::Brilliant);
    }

    #[test]
    fn progress_is_reported_for_every_position() {
        let game = parse_pgn("1. f3 e5 2. g4 Qh4# 0-1").unwrap().remove(0);
        let mut analyzer = ScriptedAnalyzer::new(fools_mate_script());
        let mut seen = Vec::new();
        review_game(
            &game,
            &mut analyzer,
            &ReviewOptions::default(),
            &OpeningBook::empty(),
            &AtomicBool::new(false),
            |p| seen.push((p.done, p.total)),
        )
        .unwrap();
        assert_eq!(seen, [(1, 5), (2, 5), (3, 5), (4, 5), (5, 5)]);
        assert_eq!(
            analyzer.calls, 4,
            "the checkmated final position is not sent to the engine"
        );
    }

    #[test]
    fn a_cancelled_review_stops_before_analysing() {
        let game = parse_pgn("1. e4 *").unwrap().remove(0);
        let mut analyzer = ScriptedAnalyzer::new(vec![]);
        let err = review_game(
            &game,
            &mut analyzer,
            &ReviewOptions::default(),
            &OpeningBook::empty(),
            &AtomicBool::new(true),
            |_| {},
        )
        .unwrap_err();
        assert_eq!(err, ReviewError::Cancelled);
        assert_eq!(analyzer.calls, 0);
    }

    #[test]
    fn engine_failures_propagate() {
        let game = parse_pgn("1. e4 *").unwrap().remove(0);
        let mut analyzer = ScriptedAnalyzer::new(vec![]);
        let err = review_game(
            &game,
            &mut analyzer,
            &ReviewOptions::default(),
            &OpeningBook::empty(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ReviewError::Engine(EngineError::NoAnalysis(_))
        ));
    }

    #[test]
    fn a_game_with_no_moves_reviews_to_an_empty_review() {
        let game = Game::from_uci_moves(None, &[], BTreeMap::new()).unwrap();
        let mut analyzer = ScriptedAnalyzer::new(vec![flat("e2e4")]);
        let review = review_game(
            &game,
            &mut analyzer,
            &ReviewOptions::default(),
            &OpeningBook::empty(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert!(review.moves.is_empty());
        assert_eq!(review.evals.len(), 1);
        assert_eq!(
            review.accuracy,
            Accuracy {
                white: None,
                black: None
            }
        );
    }

    #[test]
    fn a_stalemate_final_position_is_scored_as_equal() {
        // Stalemate in 1: Black to move has no legal moves after Qb6.
        let pgn = "[FEN \"7k/8/5K2/8/8/8/8/6Q1 w - - 0 1\"]\n\n1. Qg6 *";
        let game = parse_pgn(pgn).unwrap().remove(0);
        let mut analyzer = ScriptedAnalyzer::new(vec![pa(vec![line(1, Eval::Mate(3), "g1g7")])]);
        let review = review_game(
            &game,
            &mut analyzer,
            &ReviewOptions::default(),
            &OpeningBook::empty(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(review.evals[1], Eval::Cp(0));
        assert_eq!(
            review.moves[0].class,
            MoveClass::Blunder,
            "stalemating a won position throws the win away"
        );
    }

    #[test]
    fn moves_stream_as_soon_as_the_following_position_is_analysed() {
        let game = parse_pgn("1. f3 e5 2. g4 Qh4# 0-1").unwrap().remove(0);
        let mut analyzer = ScriptedAnalyzer::new(fools_mate_script());
        let mut events = Vec::new();
        let review = review_game_streaming(
            &game,
            &mut analyzer,
            &ReviewOptions::default(),
            &OpeningBook::empty(),
            &AtomicBool::new(false),
            |event| events.push(event),
        )
        .unwrap();

        let shape: Vec<String> = events
            .iter()
            .map(|event| match event {
                ReviewEvent::Analysed { index, total, .. } => {
                    assert_eq!(*total, 5);
                    format!("a{index}")
                }
                ReviewEvent::Move(m) => format!("m{}", m.ply),
            })
            .collect();
        assert_eq!(
            shape,
            ["a0", "a1", "m1", "a2", "m2", "a3", "m3", "a4", "m4"]
        );

        let streamed: Vec<MoveReview> = events
            .iter()
            .filter_map(|e| match e {
                ReviewEvent::Move(m) => Some(m.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(streamed, review.moves);

        let evals: Vec<Eval> = events
            .iter()
            .filter_map(|e| match e {
                ReviewEvent::Analysed { eval, .. } => Some(*eval),
                _ => None,
            })
            .collect();
        assert_eq!(evals, review.evals);
    }
}
