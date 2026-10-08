//! Evaluations, win probability and per-move accuracy.
//!
//! All `Eval` values are stored from White's point of view.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    White,
    Black,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::White => Side::Black,
            Side::Black => Side::White,
        }
    }
}

impl From<shakmaty::Color> for Side {
    fn from(color: shakmaty::Color) -> Side {
        match color {
            shakmaty::Color::White => Side::White,
            shakmaty::Color::Black => Side::Black,
        }
    }
}

/// A position evaluation from White's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Eval {
    /// Centipawns. Positive favours White.
    Cp(i32),
    /// Forced mate in `n` moves. Positive: White mates. Negative: Black mates. Never zero.
    Mate(i32),
    /// The game is over by checkmate; the payload is the winner.
    Checkmate(Side),
}

/// The kind of score Stockfish reports in a UCI `info` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UciScore {
    Cp(i32),
    Mate(i32),
}

impl Eval {
    /// Converts a UCI score (relative to the side to move) to White's point of view.
    pub fn from_uci(score: UciScore, side_to_move: Side) -> Eval {
        let sign = match side_to_move {
            Side::White => 1,
            Side::Black => -1,
        };
        match score {
            UciScore::Cp(cp) => Eval::Cp(sign * cp),
            // `mate 0` means the side to move is already checkmated.
            UciScore::Mate(0) => Eval::Checkmate(side_to_move.opposite()),
            UciScore::Mate(n) => Eval::Mate(sign * n),
        }
    }

    /// White's win probability in percent (0.0 to 100.0).
    pub fn win_percent(self) -> f64 {
        match self {
            Eval::Cp(cp) => 50.0 + 50.0 * (2.0 / (1.0 + (-0.00368208 * f64::from(cp)).exp()) - 1.0),
            Eval::Mate(n) if n > 0 => 100.0,
            Eval::Mate(_) => 0.0,
            Eval::Checkmate(Side::White) => 100.0,
            Eval::Checkmate(Side::Black) => 0.0,
        }
    }

    /// `side`'s win probability in percent.
    pub fn win_percent_for(self, side: Side) -> f64 {
        let white = self.win_percent();
        match side {
            Side::White => white,
            Side::Black => 100.0 - white,
        }
    }

    /// True if `side` has delivered or can force checkmate.
    pub fn is_mate_for(self, side: Side) -> bool {
        match (self, side) {
            (Eval::Mate(n), Side::White) => n > 0,
            (Eval::Mate(n), Side::Black) => n < 0,
            (Eval::Checkmate(winner), _) => winner == side,
            _ => false,
        }
    }

    /// True if `side` is being mated or has been mated.
    pub fn is_mate_against(self, side: Side) -> bool {
        self.is_mate_for(side.opposite())
    }

    /// Human-readable form: `+0.34`, `-1.20`, `M3`, `-M3`, `#`.
    pub fn display(self) -> String {
        match self {
            Eval::Cp(cp) => format!("{:+.2}", f64::from(cp) / 100.0),
            Eval::Mate(n) if n > 0 => format!("M{n}"),
            Eval::Mate(n) => format!("-M{}", -n),
            Eval::Checkmate(Side::White) => "1-0 #".to_string(),
            Eval::Checkmate(Side::Black) => "0-1 #".to_string(),
        }
    }
}

/// Accuracy (0 to 100) of one move, from the mover's win percentages before and after it.
/// This is the formula Lichess publishes for its accuracy metric.
pub fn move_accuracy(win_before: f64, win_after: f64) -> f64 {
    let loss = (win_before - win_after).max(0.0);
    (103.1668 * (-0.04354 * loss).exp() - 3.1668).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_position_is_fifty_percent() {
        assert!((Eval::Cp(0).win_percent() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn win_percent_is_monotonic_and_symmetric() {
        let a = Eval::Cp(100).win_percent();
        let b = Eval::Cp(300).win_percent();
        assert!(a > 50.0 && b > a);
        let neg = Eval::Cp(-100).win_percent();
        assert!((a + neg - 100.0).abs() < 1e-9);
    }

    #[test]
    fn one_pawn_is_about_fifty_nine_percent() {
        let w = Eval::Cp(100).win_percent();
        assert!((w - 59.1).abs() < 0.5, "got {w}");
    }

    #[test]
    fn mate_maps_to_the_extremes() {
        assert_eq!(Eval::Mate(3).win_percent(), 100.0);
        assert_eq!(Eval::Mate(-3).win_percent(), 0.0);
        assert_eq!(Eval::Checkmate(Side::White).win_percent(), 100.0);
        assert_eq!(Eval::Checkmate(Side::Black).win_percent(), 0.0);
    }

    #[test]
    fn win_percent_for_black_is_inverted() {
        assert!(
            (Eval::Cp(200).win_percent_for(Side::Black) - (100.0 - Eval::Cp(200).win_percent()))
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn uci_scores_are_converted_to_white_pov() {
        assert_eq!(Eval::from_uci(UciScore::Cp(50), Side::White), Eval::Cp(50));
        assert_eq!(Eval::from_uci(UciScore::Cp(50), Side::Black), Eval::Cp(-50));
        assert_eq!(
            Eval::from_uci(UciScore::Mate(2), Side::White),
            Eval::Mate(2)
        );
        assert_eq!(
            Eval::from_uci(UciScore::Mate(2), Side::Black),
            Eval::Mate(-2)
        );
        assert_eq!(
            Eval::from_uci(UciScore::Mate(-4), Side::Black),
            Eval::Mate(4)
        );
    }

    #[test]
    fn mate_zero_means_side_to_move_is_checkmated() {
        assert_eq!(
            Eval::from_uci(UciScore::Mate(0), Side::Black),
            Eval::Checkmate(Side::White)
        );
        assert_eq!(
            Eval::from_uci(UciScore::Mate(0), Side::White),
            Eval::Checkmate(Side::Black)
        );
    }

    #[test]
    fn mate_predicates() {
        assert!(Eval::Mate(2).is_mate_for(Side::White));
        assert!(!Eval::Mate(2).is_mate_for(Side::Black));
        assert!(Eval::Mate(-2).is_mate_against(Side::White));
        assert!(Eval::Checkmate(Side::Black).is_mate_for(Side::Black));
        assert!(!Eval::Cp(900).is_mate_for(Side::White));
    }

    #[test]
    fn display_formats() {
        assert_eq!(Eval::Cp(34).display(), "+0.34");
        assert_eq!(Eval::Cp(-120).display(), "-1.20");
        assert_eq!(Eval::Mate(3).display(), "M3");
        assert_eq!(Eval::Mate(-3).display(), "-M3");
    }

    #[test]
    fn accuracy_of_no_loss_is_hundred_and_decreases_with_loss() {
        assert!((move_accuracy(60.0, 60.0) - 100.0).abs() < 1e-9);
        assert!(
            (move_accuracy(60.0, 70.0) - 100.0).abs() < 1e-9,
            "gains are not penalised"
        );
        let small = move_accuracy(60.0, 55.0);
        let large = move_accuracy(60.0, 20.0);
        assert!(small < 100.0 && large < small);
        assert!(move_accuracy(100.0, 0.0) >= 0.0);
    }

    #[test]
    fn eval_serializes_with_kind_and_value() {
        let json = serde_json::to_string(&Eval::Cp(12)).unwrap();
        assert_eq!(json, r#"{"kind":"cp","value":12}"#);
        let back: Eval = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Eval::Cp(12));
        let mate = serde_json::to_string(&Eval::Checkmate(Side::White)).unwrap();
        assert_eq!(
            serde_json::from_str::<Eval>(&mate).unwrap(),
            Eval::Checkmate(Side::White)
        );
    }
}
