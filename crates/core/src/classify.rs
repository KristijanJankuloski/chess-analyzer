//! Move classification. Pure functions over numbers; no engine or board access.
//!
//! Win percentages are from the mover's point of view, in 0..=100.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::eval::{Eval, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum MoveClass {
    Book,
    Brilliant,
    Great,
    Best,
    Good,
    Inaccuracy,
    Mistake,
    Miss,
    Blunder,
}

impl MoveClass {
    /// Moves worth showing in a "critical moments" list.
    pub fn is_critical(self) -> bool {
        matches!(
            self,
            MoveClass::Brilliant
                | MoveClass::Great
                | MoveClass::Inaccuracy
                | MoveClass::Mistake
                | MoveClass::Miss
                | MoveClass::Blunder
        )
    }

    fn is_error(self) -> bool {
        matches!(
            self,
            MoveClass::Inaccuracy | MoveClass::Mistake | MoveClass::Miss | MoveClass::Blunder
        )
    }
}

/// All tunable numbers in one place. Win-percentage loss is measured in percentage points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    /// A move losing at most this much counts as the best move.
    pub best_epsilon: f64,
    /// A known opening move stays "Book" unless it loses more than this.
    pub book_max_loss: f64,
    pub good_max: f64,
    pub inaccuracy_max: f64,
    pub mistake_max: f64,
    /// A Great move must beat the second-best line by at least this much.
    pub great_gap: f64,
    /// Positions this lopsided (either way) never produce Great moves.
    pub decided_win: f64,
    /// A Brilliant move may lose at most this much.
    pub brilliant_max_loss: f64,
    /// Material the mover must be down (in pawns) two plies later to call it a sacrifice.
    pub brilliant_min_sacrifice: i32,
    /// A Brilliant move must leave the mover at least this well off.
    pub brilliant_min_win_after: f64,
    /// A Brilliant move is not awarded if the mover was already this far ahead.
    pub brilliant_max_win_before: f64,
    /// For Miss: the mover must have had at least this win percentage before the move.
    /// Also the line between a Miss and a Blunder when a forced mate is dropped: if the
    /// mover is still at least this well off afterwards it is a Miss.
    pub miss_min_win_before: f64,
}

impl Default for Thresholds {
    fn default() -> Thresholds {
        Thresholds {
            best_epsilon: 0.5,
            book_max_loss: 10.0,
            good_max: 5.0,
            inaccuracy_max: 10.0,
            mistake_max: 20.0,
            great_gap: 12.0,
            decided_win: 97.0,
            brilliant_max_loss: 2.0,
            brilliant_min_sacrifice: 2,
            brilliant_min_win_after: 50.0,
            brilliant_max_win_before: 90.0,
            miss_min_win_before: 60.0,
        }
    }
}

/// Everything the classifier needs to know about one move.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveContext {
    pub mover: Side,
    pub played_uci: String,
    pub best_uci: Option<String>,
    /// Mover's win % in the position before the move (engine's best line).
    pub win_before: f64,
    /// Mover's win % for the engine's second-best line, if there is one.
    pub win_second: Option<f64>,
    /// Mover's win % after the move that was played.
    pub win_after: f64,
    pub eval_before: Eval,
    pub eval_after: Eval,
    /// The position after this move is still within a known opening line.
    pub in_book: bool,
    pub prev_opponent_class: Option<MoveClass>,
    /// Mover's material change, in pawns, from before the move to after the opponent's
    /// best reply. Negative means material was given up.
    pub material_swing: i32,
}

impl MoveContext {
    pub fn loss(&self) -> f64 {
        if self.best_uci.as_deref() == Some(self.played_uci.as_str()) {
            0.0
        } else {
            (self.win_before - self.win_after).max(0.0)
        }
    }
}

pub fn classify(ctx: &MoveContext, t: &Thresholds) -> MoveClass {
    let loss = ctx.loss();
    if ctx.in_book && loss <= t.book_max_loss {
        return MoveClass::Book;
    }

    // Mate handling comes first: it overrides the percentage-based rules.
    if ctx.eval_after.is_mate_against(ctx.mover) && !ctx.eval_before.is_mate_against(ctx.mover) {
        return MoveClass::Blunder;
    }
    if ctx.eval_before.is_mate_for(ctx.mover) && !ctx.eval_after.is_mate_for(ctx.mover) {
        return if ctx.win_after < t.miss_min_win_before {
            MoveClass::Blunder
        } else {
            MoveClass::Miss
        };
    }

    let is_best = loss <= t.best_epsilon;

    if loss <= t.brilliant_max_loss
        && ctx.material_swing <= -t.brilliant_min_sacrifice
        && ctx.win_after >= t.brilliant_min_win_after
        && ctx.win_before <= t.brilliant_max_win_before
    {
        return MoveClass::Brilliant;
    }

    if is_best {
        let decided = ctx.win_before >= t.decided_win || ctx.win_before <= 100.0 - t.decided_win;
        if let Some(second) = ctx.win_second
            && !decided
            && ctx.win_before - second >= t.great_gap
        {
            return MoveClass::Great;
        }
        return MoveClass::Best;
    }

    let base = if loss <= t.good_max {
        MoveClass::Good
    } else if loss <= t.inaccuracy_max {
        MoveClass::Inaccuracy
    } else if loss <= t.mistake_max {
        MoveClass::Mistake
    } else {
        MoveClass::Blunder
    };

    let opponent_erred =
        matches!(ctx.prev_opponent_class, Some(c) if c.is_error() && c != MoveClass::Inaccuracy);
    if matches!(base, MoveClass::Inaccuracy | MoveClass::Mistake)
        && opponent_erred
        && ctx.win_before >= t.miss_min_win_before
    {
        return MoveClass::Miss;
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MoveContext {
        MoveContext {
            mover: Side::White,
            played_uci: "e2e4".into(),
            best_uci: Some("d2d4".into()),
            win_before: 50.0,
            win_second: None,
            win_after: 50.0,
            eval_before: Eval::Cp(0),
            eval_after: Eval::Cp(0),
            in_book: false,
            prev_opponent_class: None,
            material_swing: 0,
        }
    }

    fn with_loss(loss: f64) -> MoveContext {
        MoveContext {
            win_after: 50.0 - loss,
            ..ctx()
        }
    }

    #[test]
    fn playing_the_engine_move_is_best_even_if_numbers_differ() {
        let c = MoveContext {
            played_uci: "d2d4".into(),
            win_after: 30.0,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Best);
    }

    #[test]
    fn loss_bands() {
        let t = Thresholds::default();
        assert_eq!(classify(&with_loss(0.3), &t), MoveClass::Best);
        assert_eq!(classify(&with_loss(3.0), &t), MoveClass::Good);
        assert_eq!(classify(&with_loss(8.0), &t), MoveClass::Inaccuracy);
        assert_eq!(classify(&with_loss(15.0), &t), MoveClass::Mistake);
        assert_eq!(classify(&with_loss(35.0), &t), MoveClass::Blunder);
    }

    #[test]
    fn thresholds_are_configurable() {
        let strict = Thresholds {
            good_max: 1.0,
            ..Thresholds::default()
        };
        assert_eq!(classify(&with_loss(3.0), &strict), MoveClass::Inaccuracy);
    }

    #[test]
    fn book_moves_stay_book_even_with_a_small_loss() {
        let c = MoveContext {
            in_book: true,
            ..with_loss(6.0)
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Book);
    }

    #[test]
    fn a_book_move_that_loses_badly_is_not_book() {
        // The Fool's Mate is a named line in the opening data, but 2. g4?? is still a blunder.
        let c = MoveContext {
            in_book: true,
            ..with_loss(40.0)
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Blunder);
        let into_mate = MoveContext {
            in_book: true,
            eval_after: Eval::Mate(-1),
            win_after: 0.0,
            ..ctx()
        };
        assert_eq!(
            classify(&into_mate, &Thresholds::default()),
            MoveClass::Blunder
        );
    }

    #[test]
    fn walking_into_mate_is_a_blunder() {
        let c = MoveContext {
            eval_after: Eval::Mate(-2),
            win_after: 0.0,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Blunder);
    }

    #[test]
    fn already_being_mated_is_judged_by_loss_not_flagged_again() {
        let c = MoveContext {
            eval_before: Eval::Mate(-3),
            eval_after: Eval::Mate(-2),
            win_before: 0.0,
            win_after: 0.0,
            played_uci: "a2a3".into(),
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Best);
    }

    #[test]
    fn slower_mate_is_not_penalised() {
        let c = MoveContext {
            eval_before: Eval::Mate(2),
            eval_after: Eval::Mate(5),
            win_before: 100.0,
            win_after: 100.0,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Best);
    }

    #[test]
    fn dropping_a_forced_mate_but_staying_winning_is_a_miss() {
        let c = MoveContext {
            eval_before: Eval::Mate(2),
            eval_after: Eval::Cp(500),
            win_before: 100.0,
            win_after: 90.0,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Miss);
    }

    #[test]
    fn dropping_a_forced_mate_into_a_worse_position_is_a_blunder() {
        let c = MoveContext {
            eval_before: Eval::Mate(2),
            eval_after: Eval::Cp(-200),
            win_before: 100.0,
            win_after: 30.0,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Blunder);
    }

    #[test]
    fn delivering_checkmate_is_best() {
        let c = MoveContext {
            eval_before: Eval::Mate(1),
            eval_after: Eval::Checkmate(Side::White),
            win_before: 100.0,
            win_after: 100.0,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Best);
    }

    #[test]
    fn mate_logic_respects_the_mover_side() {
        let c = MoveContext {
            mover: Side::Black,
            eval_before: Eval::Mate(-2),
            eval_after: Eval::Cp(-500),
            win_before: 100.0,
            win_after: 90.0,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Miss);
    }

    #[test]
    fn inaccuracy_after_opponent_blunder_becomes_a_miss() {
        let c = MoveContext {
            win_before: 80.0,
            win_after: 68.0,
            prev_opponent_class: Some(MoveClass::Blunder),
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Miss);
    }

    #[test]
    fn miss_needs_a_prior_advantage() {
        let c = MoveContext {
            win_before: 52.0,
            win_after: 40.0,
            prev_opponent_class: Some(MoveClass::Blunder),
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Mistake);
    }

    #[test]
    fn a_blunder_stays_a_blunder_after_an_opponent_error() {
        let c = MoveContext {
            win_before: 80.0,
            win_after: 30.0,
            prev_opponent_class: Some(MoveClass::Blunder),
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Blunder);
    }

    #[test]
    fn only_move_in_a_close_position_is_great() {
        let c = MoveContext {
            played_uci: "d2d4".into(),
            win_before: 55.0,
            win_second: Some(30.0),
            win_after: 55.0,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Great);
    }

    #[test]
    fn great_needs_a_big_gap_and_an_undecided_position() {
        let t = Thresholds::default();
        let small_gap = MoveContext {
            played_uci: "d2d4".into(),
            win_before: 55.0,
            win_second: Some(50.0),
            win_after: 55.0,
            ..ctx()
        };
        assert_eq!(classify(&small_gap, &t), MoveClass::Best);
        let decided = MoveContext {
            played_uci: "d2d4".into(),
            win_before: 99.0,
            win_second: Some(60.0),
            win_after: 99.0,
            ..ctx()
        };
        assert_eq!(classify(&decided, &t), MoveClass::Best);
        let lost = MoveContext {
            played_uci: "d2d4".into(),
            win_before: 2.0,
            win_second: Some(0.0),
            win_after: 2.0,
            ..ctx()
        };
        assert_eq!(classify(&lost, &t), MoveClass::Best);
    }

    #[test]
    fn sound_sacrifice_is_brilliant() {
        let c = MoveContext {
            played_uci: "d2d4".into(),
            win_before: 55.0,
            win_after: 60.0,
            material_swing: -3,
            ..ctx()
        };
        assert_eq!(classify(&c, &Thresholds::default()), MoveClass::Brilliant);
    }

    #[test]
    fn unsound_or_unneeded_sacrifices_are_not_brilliant() {
        let t = Thresholds::default();
        let losing = MoveContext {
            win_before: 55.0,
            win_after: 30.0,
            material_swing: -3,
            ..ctx()
        };
        assert_ne!(classify(&losing, &t), MoveClass::Brilliant);
        let already_winning = MoveContext {
            played_uci: "d2d4".into(),
            win_before: 95.0,
            win_after: 96.0,
            material_swing: -3,
            ..ctx()
        };
        assert_eq!(classify(&already_winning, &t), MoveClass::Best);
        let no_sacrifice = MoveContext {
            played_uci: "d2d4".into(),
            win_before: 55.0,
            win_after: 60.0,
            ..ctx()
        };
        assert_eq!(classify(&no_sacrifice, &t), MoveClass::Best);
    }

    #[test]
    fn critical_classes() {
        assert!(MoveClass::Blunder.is_critical());
        assert!(MoveClass::Brilliant.is_critical());
        assert!(MoveClass::Miss.is_critical());
        assert!(!MoveClass::Best.is_critical());
        assert!(!MoveClass::Good.is_critical());
        assert!(!MoveClass::Book.is_critical());
    }
}
