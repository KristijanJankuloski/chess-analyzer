//! Plain-language commentary on a move, written from a `Digest`.
//!
//! This is a template renderer: it chooses sentences, never chess. Everything it says comes from
//! the digest, so it cannot name a piece, square or move the engine's lines did not contain. When
//! the digest holds no fact that explains a move, it says only how the evaluation changed and
//! which move was better, and never invents a cause.
//!
//! All wording is in this one module, in English.

use crate::classify::MoveClass;
use crate::eval::{Eval, Side};
use crate::facts::{CommentaryInput, Digest, Fact, MoveFacts, Spot, digest};

/// The commentary for the move in `input`, or `None` if its facts could not be worked out.
pub fn write(input: &CommentaryInput<'_>) -> Option<String> {
    digest(input).map(|d| render(&d))
}

/// Chooses between equivalent phrasings by ply, so a game does not read identically while the
/// text for a given move stays the same every time.
fn pick<'a>(options: &[&'a str], ply: usize) -> &'a str {
    options[ply % options.len()]
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::White => "White",
        Side::Black => "Black",
    }
}

fn points(n: i32) -> String {
    if n == 1 {
        "1 point".to_string()
    } else {
        format!("{n} points")
    }
}

/// "the knight on f6", or "the pawns on b7 and f7" when all are the same kind.
fn describe_spots(spots: &[Spot]) -> String {
    let Some(first) = spots.first() else {
        return String::new();
    };
    if spots.iter().all(|s| s.kind == first.kind) && spots.len() > 1 {
        let squares: Vec<&str> = spots.iter().map(|s| s.square.as_str()).collect();
        return format!("the {}s on {}", first.kind.name(), join(&squares));
    }
    let each: Vec<String> = spots
        .iter()
        .map(|s| format!("the {} on {}", s.kind.name(), s.square))
        .collect();
    join(&each)
}

/// "a", "a and b", "a, b and c".
fn join<S: AsRef<str>>(items: &[S]) -> String {
    match items {
        [] => String::new(),
        [only] => only.as_ref().to_string(),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(", "),
            last.as_ref()
        ),
    }
}

/// How an evaluation reads in a sentence.
fn eval_phrase(eval: Eval) -> String {
    match eval {
        Eval::Mate(n) if n > 0 => format!("a forced mate in {n} for White"),
        Eval::Mate(n) => format!("a forced mate in {} for Black", n.unsigned_abs()),
        Eval::Checkmate(winner) => format!("checkmate, won by {}", side_name(winner)),
        Eval::Cp(cp) => {
            let side = if cp > 0 { "White" } else { "Black" };
            match cp.unsigned_abs() {
                0..50 => "equal".to_string(),
                50..150 => format!("slightly better for {side}"),
                150..300 => format!("clearly better for {side}"),
                _ => format!("winning for {side}"),
            }
        }
    }
}

fn is_error(class: MoveClass) -> bool {
    matches!(
        class,
        MoveClass::Inaccuracy | MoveClass::Mistake | MoveClass::Miss | MoveClass::Blunder
    )
}

/// The first sentence's wording for a class, with the move written in.
fn verdict(d: &Digest) -> String {
    let ply = d.ply;
    let phrase = match d.class {
        MoveClass::Brilliant => pick(&["is a brilliant move", "is brilliant"], ply),
        MoveClass::Great => pick(&["is a great move", "is a great find"], ply),
        MoveClass::Best => pick(&["is the best move", "is the engine's top choice"], ply),
        MoveClass::Good => "is a good move",
        MoveClass::Book => "is a book move",
        MoveClass::Inaccuracy => pick(&["is an inaccuracy", "was an inaccuracy"], ply),
        MoveClass::Mistake => pick(&["is a mistake", "was a mistake"], ply),
        MoveClass::Miss => pick(&["is a miss", "was a miss"], ply),
        MoveClass::Blunder => pick(&["is a blunder", "was a blunder"], ply),
    };
    format!("{} {phrase}", d.san)
}

/// One sentence for a fact, in the voice of the player who moved.
fn sentence(fact: &Fact, d: &Digest) -> String {
    let mover = side_name(d.mover);
    let opponent = side_name(d.mover.opposite());
    match fact {
        Fact::MateAllowed { moves } => {
            format!("It allows {opponent} to force checkmate in {moves}.")
        }
        Fact::MateMissed { moves } => format!("It gives up a forced checkmate in {moves}."),
        Fact::ForcesMate { moves } => format!("It leads to a forced checkmate in {moves}."),
        Fact::MaterialLost { points: n } => {
            let better = d.best_san.as_deref().unwrap_or("the best move");
            format!(
                "It costs {} of material compared with {better}.",
                points(*n)
            )
        }
        Fact::WinsMaterial { points: n } => {
            format!("It wins {} of material in the engine's line.", points(*n))
        }
        Fact::Loose { reply, pieces } => {
            let verb = if pieces.len() > 1 { "are both" } else { "is" };
            format!(
                "After {reply}, {} {verb} attacked and short of protection.",
                describe_spots(pieces)
            )
        }
        Fact::ForcedKingMove {
            line,
            square,
            loses_castling,
        } => {
            let castling = if *loses_castling {
                " and can no longer castle"
            } else {
                ""
            };
            format!(
                "In the engine's line {}, {mover}'s king is driven to {square}{castling}.",
                line.join(" ")
            )
        }
        Fact::AllowsFork { reply, targets, .. } => {
            format!(
                "It allows {reply}, which forks {}.",
                describe_spots(targets)
            )
        }
        Fact::AllowsPin { reply, pinned, .. } => format!(
            "It allows {reply}, which pins {} to the king.",
            describe_spots(std::slice::from_ref(pinned))
        ),
        Fact::Forks { targets, .. } => format!("It forks {}.", describe_spots(targets)),
        Fact::Pins { pinned, .. } => format!(
            "It pins {} to the king.",
            describe_spots(std::slice::from_ref(pinned))
        ),
    }
}

/// What a move does by itself, when no larger fact says it better. Mating is always worth
/// saying; captures and checks only for the moves singled out as brilliant or great.
fn describe_move(played: &MoveFacts, notable: bool) -> Option<String> {
    if played.mate {
        Some("It delivers checkmate.".to_string())
    } else if !notable {
        None
    } else if let Some(kind) = played.captures {
        Some(format!("It captures the {}.", kind.name()))
    } else if played.check {
        Some("It gives check.".to_string())
    } else {
        None
    }
}

/// Two or three plain sentences about the move.
pub fn render(d: &Digest) -> String {
    let better = d
        .best_san
        .as_deref()
        .filter(|_| !d.played_is_best)
        .map(str::to_string);

    if is_error(d.class) {
        let mut first = verdict(d);
        match &better {
            Some(best) => first.push_str(&format!("; {best} was better.")),
            None => first.push('.'),
        }
        let causes: Vec<String> = d
            .facts
            .iter()
            .filter(|f| f.is_consequence())
            .take(2)
            .map(|f| sentence(f, d))
            .collect();
        if causes.is_empty() {
            let (before, after) = (eval_phrase(d.eval_before), eval_phrase(d.eval_after));
            // Two equal phrases would read as no change at all, so give the numbers instead.
            return if before == after {
                format!(
                    "{first} The evaluation goes from {} to {} (from White's point of view).",
                    d.eval_before.display(),
                    d.eval_after.display()
                )
            } else {
                format!("{first} The evaluation goes from {before} to {after}.")
            };
        }
        return format!("{first} {}", causes.join(" "));
    }

    let mut text = format!("{}.", verdict(d));
    if d.class == MoveClass::Good
        && let Some(best) = &better
    {
        text.push_str(&format!(" Best was {best}."));
    }
    let achievement = d
        .played
        .mate
        .then(|| "It delivers checkmate.".to_string())
        .or_else(|| {
            d.facts
                .iter()
                .find(|f| !f.is_consequence())
                .map(|f| sentence(f, d))
        })
        .or_else(|| {
            let notable = matches!(d.class, MoveClass::Brilliant | MoveClass::Great);
            describe_move(&d.played, notable)
        });
    if let Some(achievement) = achievement {
        text.push(' ');
        text.push_str(&achievement);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{Kind, MoveFacts};

    fn quiet() -> MoveFacts {
        MoveFacts {
            kind: Kind::Knight,
            captures: None,
            check: false,
            mate: false,
            castles: false,
            promotes: None,
        }
    }

    fn base(class: MoveClass) -> Digest {
        Digest {
            ply: 12,
            mover: Side::Black,
            san: "Nf6".into(),
            class,
            loss: 13.8,
            eval_before: Eval::Cp(146),
            eval_after: Eval::Cp(327),
            played_is_best: false,
            best_san: Some("Qf6".into()),
            played: quiet(),
            best: None,
            played_line: vec![],
            best_line: vec![],
            material_now: 0,
            material_played: 0,
            material_best: None,
            facts: vec![],
        }
    }

    fn spot(kind: Kind, square: &str) -> Spot {
        Spot {
            kind,
            square: square.into(),
        }
    }

    #[test]
    fn a_mistake_names_the_better_move_and_the_strongest_causes() {
        let mut d = base(MoveClass::Mistake);
        d.facts = vec![
            Fact::Loose {
                reply: "Qb3".into(),
                pieces: vec![spot(Kind::Pawn, "b7"), spot(Kind::Pawn, "f7")],
            },
            Fact::ForcedKingMove {
                line: vec!["Qb3".into(), "Bc5".into(), "Bxf7+".into(), "Ke7".into()],
                square: "e7".into(),
                loses_castling: true,
            },
        ];
        assert_eq!(
            render(&d),
            "Nf6 is a mistake; Qf6 was better. \
             After Qb3, the pawns on b7 and f7 are both attacked and short of protection. \
             In the engine's line Qb3 Bc5 Bxf7+ Ke7, Black's king is driven to e7 and can no longer castle."
        );
    }

    #[test]
    fn only_the_two_strongest_causes_are_used() {
        let mut d = base(MoveClass::Blunder);
        d.facts = vec![
            Fact::MateAllowed { moves: 2 },
            Fact::MaterialLost { points: 3 },
            Fact::AllowsPin {
                reply: "Bb5".into(),
                slider: Kind::Bishop,
                pinned: spot(Kind::Knight, "c6"),
            },
        ];
        let text = render(&d);
        assert!(text.contains("It allows White to force checkmate in 2."));
        assert!(text.contains("It costs 3 points of material compared with Qf6."));
        assert!(!text.contains("pins"));
    }

    #[test]
    fn without_a_cause_it_states_only_what_the_engine_shows() {
        let d = base(MoveClass::Inaccuracy);
        assert_eq!(
            render(&d),
            "Nf6 is an inaccuracy; Qf6 was better. \
             The evaluation goes from slightly better for White to winning for White."
        );
    }

    #[test]
    fn colours_follow_the_mover() {
        let mut d = base(MoveClass::Blunder);
        d.mover = Side::White;
        d.facts = vec![Fact::MateAllowed { moves: 1 }];
        assert!(render(&d).contains("It allows Black to force checkmate in 1."));
        d.mover = Side::Black;
        assert!(render(&d).contains("It allows White to force checkmate in 1."));
    }

    #[test]
    fn a_missing_best_move_is_not_invented() {
        let mut d = base(MoveClass::Mistake);
        d.best_san = None;
        assert!(render(&d).starts_with("Nf6 is a mistake. "));
        assert!(!render(&d).contains("was better"));
    }

    #[test]
    fn the_wording_varies_with_the_ply_but_not_between_calls() {
        let mut d = base(MoveClass::Mistake);
        d.ply = 12;
        let even = render(&d);
        d.ply = 13;
        let odd = render(&d);
        assert_ne!(even, odd);
        d.ply = 12;
        assert_eq!(render(&d), even);
    }

    #[test]
    fn praise_mentions_what_the_move_wins_or_does() {
        let mut d = base(MoveClass::Best);
        d.san = "Qxd5+".into();
        d.played_is_best = true;
        d.facts = vec![Fact::WinsMaterial { points: 3 }];
        d.ply = 2;
        assert_eq!(
            render(&d),
            "Qxd5+ is the best move. It wins 3 points of material in the engine's line."
        );

        let mut great = base(MoveClass::Great);
        great.ply = 2;
        great.played.captures = Some(Kind::Bishop);
        great.played_is_best = true;
        assert_eq!(
            render(&great),
            "Nf6 is a great move. It captures the bishop."
        );
    }

    #[test]
    fn a_mating_move_says_so_whatever_its_class() {
        let mut d = base(MoveClass::Best);
        d.san = "Qh4#".into();
        d.ply = 2;
        d.played_is_best = true;
        d.played.mate = true;
        assert_eq!(render(&d), "Qh4# is the best move. It delivers checkmate.");
        d.class = MoveClass::Book;
        assert_eq!(render(&d), "Qh4# is a book move. It delivers checkmate.");
    }

    #[test]
    fn a_mating_move_says_checkmate_even_when_a_fork_is_on_the_digest() {
        let mut d = base(MoveClass::Best);
        d.san = "Rd8#".into();
        d.ply = 2;
        d.played_is_best = true;
        d.played.mate = true;
        d.facts = vec![Fact::Forks {
            attacker: Kind::Rook,
            targets: vec![spot(Kind::King, "h8"), spot(Kind::Knight, "d3")],
        }];
        assert_eq!(render(&d), "Rd8# is the best move. It delivers checkmate.");
    }

    #[test]
    fn when_both_evaluations_read_alike_the_numbers_are_given() {
        let mut d = base(MoveClass::Inaccuracy);
        d.eval_before = Eval::Cp(200);
        d.eval_after = Eval::Cp(250);
        assert_eq!(
            render(&d),
            "Nf6 is an inaccuracy; Qf6 was better. The evaluation goes from +2.00 to +2.50 (from White's point of view)."
        );
    }

    #[test]
    fn a_plain_good_move_points_to_the_best_one_and_a_book_move_says_only_that() {
        let d = base(MoveClass::Good);
        assert_eq!(render(&d), "Nf6 is a good move. Best was Qf6.");
        let book = base(MoveClass::Book);
        assert_eq!(render(&book), "Nf6 is a book move.");
    }

    #[test]
    fn forks_and_pins_read_naturally() {
        let mut d = base(MoveClass::Mistake);
        d.facts = vec![Fact::AllowsFork {
            reply: "Nb6+".into(),
            attacker: Kind::Knight,
            targets: vec![spot(Kind::Rook, "a8"), spot(Kind::King, "d7")],
        }];
        assert!(
            render(&d).contains("It allows Nb6+, which forks the rook on a8 and the king on d7.")
        );
        d.facts = vec![Fact::AllowsPin {
            reply: "Re1".into(),
            slider: Kind::Rook,
            pinned: spot(Kind::Knight, "e7"),
        }];
        assert!(render(&d).contains("It allows Re1, which pins the knight on e7 to the king."));

        let mut good = base(MoveClass::Great);
        good.played_is_best = true;
        good.facts = vec![Fact::Forks {
            attacker: Kind::Knight,
            targets: vec![spot(Kind::Rook, "a8"), spot(Kind::King, "e8")],
        }];
        assert!(render(&good).ends_with("It forks the rook on a8 and the king on e8."));
        good.facts = vec![Fact::Pins {
            slider: Kind::Rook,
            pinned: spot(Kind::Knight, "e7"),
        }];
        assert!(render(&good).ends_with("It pins the knight on e7 to the king."));
    }

    #[test]
    fn evaluations_read_naturally() {
        assert_eq!(eval_phrase(Eval::Cp(10)), "equal");
        assert_eq!(eval_phrase(Eval::Cp(-120)), "slightly better for Black");
        assert_eq!(eval_phrase(Eval::Cp(200)), "clearly better for White");
        assert_eq!(eval_phrase(Eval::Cp(-900)), "winning for Black");
        assert_eq!(eval_phrase(Eval::Mate(-3)), "a forced mate in 3 for Black");
        assert_eq!(
            eval_phrase(Eval::Checkmate(Side::White)),
            "checkmate, won by White"
        );
    }

    #[test]
    fn spots_are_listed_naturally() {
        assert_eq!(
            describe_spots(&[spot(Kind::Knight, "f6")]),
            "the knight on f6"
        );
        assert_eq!(
            describe_spots(&[spot(Kind::Rook, "a8"), spot(Kind::King, "e8")]),
            "the rook on a8 and the king on e8"
        );
        assert_eq!(
            describe_spots(&[spot(Kind::Pawn, "b7"), spot(Kind::Pawn, "f7")]),
            "the pawns on b7 and f7"
        );
    }
}
