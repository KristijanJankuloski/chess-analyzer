//! Plain-text rendering of a `Review`.

use chess_analyzer_core::classify::MoveClass;
use chess_analyzer_core::eval::Side;
use chess_analyzer_core::game::Game;
use chess_analyzer_core::review::{MoveReview, Review, digest_for};

fn class_label(class: MoveClass) -> &'static str {
    match class {
        MoveClass::Book => "Book",
        MoveClass::Brilliant => "Brilliant",
        MoveClass::Great => "Great",
        MoveClass::Best => "Best",
        MoveClass::Good => "Good",
        MoveClass::Inaccuracy => "Inaccuracy",
        MoveClass::Mistake => "Mistake",
        MoveClass::Miss => "Miss",
        MoveClass::Blunder => "Blunder",
    }
}

fn move_label(m: &MoveReview) -> String {
    match m.side {
        Side::White => format!("{}. {}", m.move_number, m.san),
        Side::Black => format!("{}... {}", m.move_number, m.san),
    }
}

fn accuracy(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_string(), |v| format!("{v:.1}"))
}

pub fn render(review: &Review) -> String {
    let header = |key: &str| review.headers.get(key).map(String::as_str).unwrap_or("?");
    let mut out = String::new();
    out.push_str(&format!(
        "{} vs {}  ({})\n",
        header("White"),
        header("Black"),
        header("Result")
    ));
    match &review.opening {
        Some(o) => out.push_str(&format!("Opening: {} {}\n", o.eco, o.name)),
        None => out.push_str("Opening: not in the book\n"),
    }
    out.push_str(&format!(
        "Engine: {} (depth {}, {} lines)\n",
        review.engine, review.limits.depth, review.limits.multipv
    ));
    out.push_str(&format!(
        "Accuracy: White {} | Black {}\n\n",
        accuracy(review.accuracy.white),
        accuracy(review.accuracy.black)
    ));

    out.push_str("Moves\n");
    for m in &review.moves {
        out.push_str(&format!(
            "  {:<14} {:<11} {:>8}\n",
            move_label(m),
            class_label(m.class),
            m.eval_after.display()
        ));
    }

    out.push_str("\nCritical moments\n");
    if review.critical_plies.is_empty() {
        out.push_str("  none\n");
    }
    for &ply in &review.critical_plies {
        let m = &review.moves[ply - 1];
        let best = match (&m.best_san, m.class) {
            (Some(san), c) if !matches!(c, MoveClass::Brilliant | MoveClass::Great) => {
                format!(", best was {san}")
            }
            _ => String::new(),
        };
        out.push_str(&format!(
            "  {:<14} {:<11} {} -> {}{} (lost {:.1}% win chance)\n",
            move_label(m),
            class_label(m.class),
            m.eval_before.display(),
            m.eval_after.display(),
            best,
            m.loss
        ));
    }
    out
}

/// The commentary under each critical move, as shown in the app. With `show_facts`, the ranked
/// facts it was written from follow each sentence, as JSON, for checking wording and detectors
/// against real games.
pub fn render_commentary(game: &Game, review: &Review, show_facts: bool) -> String {
    let mut out = String::from("\nCommentary\n");
    if review.critical_plies.is_empty() {
        out.push_str("  none\n");
    }
    for &ply in &review.critical_plies {
        let m = &review.moves[ply - 1];
        out.push_str(&format!(
            "  {:<14} {}\n",
            move_label(m),
            class_label(m.class)
        ));
        out.push_str(&format!(
            "      {}\n",
            m.commentary.as_deref().unwrap_or("(no commentary)")
        ));
        if show_facts && let Some(digest) = digest_for(game, review, ply - 1) {
            let facts = serde_json::to_string(&digest.facts).unwrap_or_default();
            out.push_str(&format!("      facts: {facts}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_analyzer_core::engine::Limits;
    use chess_analyzer_core::eval::Eval;
    use chess_analyzer_core::openings::Opening;
    use chess_analyzer_core::review::Accuracy;
    use std::collections::BTreeMap;

    fn mv(
        ply: usize,
        side: Side,
        san: &str,
        class: MoveClass,
        before: Eval,
        after: Eval,
        loss: f64,
    ) -> MoveReview {
        MoveReview {
            ply,
            move_number: ply.div_ceil(2) as u32,
            side,
            san: san.to_string(),
            uci: String::new(),
            class,
            eval_before: before,
            eval_after: after,
            best_uci: Some("d2d4".into()),
            best_san: Some("d4".into()),
            best_pv: vec![],
            loss,
            accuracy: 50.0,
            critical: class.is_critical(),
            commentary: None,
        }
    }

    fn sample() -> Review {
        let mut headers = BTreeMap::new();
        headers.insert("White".to_string(), "Alice".to_string());
        headers.insert("Black".to_string(), "Bob".to_string());
        headers.insert("Result".to_string(), "0-1".to_string());
        Review {
            headers,
            opening: Some(Opening {
                eco: "C20".into(),
                name: "King's Pawn Game".into(),
            }),
            engine: "Stockfish 19".into(),
            limits: Limits {
                depth: 20,
                multipv: 3,
            },
            evals: vec![],
            moves: vec![
                mv(
                    1,
                    Side::White,
                    "e4",
                    MoveClass::Book,
                    Eval::Cp(20),
                    Eval::Cp(20),
                    0.0,
                ),
                mv(
                    2,
                    Side::Black,
                    "g5",
                    MoveClass::Blunder,
                    Eval::Cp(20),
                    Eval::Mate(3),
                    40.0,
                ),
            ],
            accuracy: Accuracy {
                white: Some(95.5),
                black: None,
            },
            critical_plies: vec![2],
        }
    }

    #[test]
    fn renders_header_moves_and_critical_moments() {
        let text = render(&sample());
        assert!(text.contains("Alice vs Bob  (0-1)"));
        assert!(text.contains("Opening: C20 King's Pawn Game"));
        assert!(text.contains("Accuracy: White 95.5 | Black n/a"));
        assert!(text.contains("1. e4"));
        assert!(text.contains("1... g5"));
        let critical = text.split("Critical moments").nth(1).unwrap();
        assert!(critical.contains("1... g5"));
        assert!(critical.contains("+0.20 -> M3"));
        assert!(critical.contains("best was d4"));
        assert!(!critical.contains("1. e4"));
    }

    fn opera_game_review() -> (Game, Review) {
        use chess_analyzer_core::engine::{PositionAnalysis, ScriptedAnalyzer};
        use chess_analyzer_core::game::parse_pgn;
        use chess_analyzer_core::openings::OpeningBook;
        use chess_analyzer_core::review::{ReviewOptions, review_game};
        use std::sync::atomic::AtomicBool;
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures");
        let pgn = std::fs::read_to_string(fixtures.join("opera_game.pgn")).unwrap();
        let recorded = std::fs::read_to_string(fixtures.join("opera_game.analysis.json")).unwrap();
        let analyses: Vec<PositionAnalysis> = serde_json::from_str(&recorded).unwrap();
        let game = parse_pgn(&pgn).unwrap().remove(0);
        let review = review_game(
            &game,
            &mut ScriptedAnalyzer::new(analyses),
            &ReviewOptions::default(),
            OpeningBook::bundled(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        (game, review)
    }

    #[test]
    fn commentary_follows_each_critical_move_and_facts_are_opt_in() {
        let (game, review) = opera_game_review();
        let plain = render_commentary(&game, &review, false);
        assert!(
            plain.contains("Nf6 is a mistake; Qf6 was better."),
            "{plain}"
        );
        assert!(!plain.contains("facts:"));
        let detailed = render_commentary(&game, &review, true);
        assert!(
            detailed.contains(r#"facts: [{"fact":"loose""#),
            "{detailed}"
        );
    }

    #[test]
    fn no_critical_moments_says_none() {
        let mut review = sample();
        review.critical_plies.clear();
        assert!(render(&review).contains("Critical moments\n  none"));
    }

    #[test]
    fn missing_headers_render_as_question_marks() {
        let mut review = sample();
        review.headers.clear();
        assert!(render(&review).starts_with("? vs ?  (?)"));
    }
}
