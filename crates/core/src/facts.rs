//! Facts about one move, worked out from the engine's own lines.
//!
//! `digest` replays the played move and the engine's principal variations on a real board and
//! lists what can be verified: material along the lines, forced mates, pieces left short of
//! protection, a king driven out of castling, forks and pins. Every renderer of commentary (the
//! template text in `commentary`, and a language model later) works from this list and nothing
//! else, so none of them ever has to read a board or judge a move.
//!
//! Each fact is optional. A line that stops being legal simply ends there, and a fact that cannot
//! be established is left out rather than guessed.

use serde::Serialize;
use shakmaty::fen::Fen;
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, Color, Move, Position, Role};

use crate::classify::MoveClass;
use crate::eval::{Eval, Side};
use crate::review::{MoveReview, balance};

mod exchange;
mod king;
mod motifs;

/// How many plies of an engine line are replayed. Lines are compared at the same length, and an
/// even length, so that both end after the opponent's move.
const MAX_PLIES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Pawn => "pawn",
            Kind::Knight => "knight",
            Kind::Bishop => "bishop",
            Kind::Rook => "rook",
            Kind::Queen => "queen",
            Kind::King => "king",
        }
    }
}

impl From<Role> for Kind {
    fn from(role: Role) -> Kind {
        match role {
            Role::Pawn => Kind::Pawn,
            Role::Knight => Kind::Knight,
            Role::Bishop => Kind::Bishop,
            Role::Rook => Kind::Rook,
            Role::Queen => Kind::Queen,
            Role::King => Kind::King,
        }
    }
}

/// A piece on a square, e.g. the pawn on f7.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Spot {
    pub kind: Kind,
    pub square: String,
}

fn spot(role: Role, square: shakmaty::Square) -> Spot {
    Spot {
        kind: role.into(),
        square: square.to_string(),
    }
}

/// What a move does, taken on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoveFacts {
    pub kind: Kind,
    pub captures: Option<Kind>,
    pub check: bool,
    pub mate: bool,
    pub castles: bool,
    pub promotes: Option<Kind>,
}

fn move_facts(position: &Chess, mv: Move) -> MoveFacts {
    let mut after = position.clone();
    after.play_unchecked(mv);
    MoveFacts {
        kind: mv.role().into(),
        captures: mv.capture().map(Kind::from),
        check: after.is_check(),
        mate: after.is_checkmate(),
        castles: mv.is_castle(),
        promotes: mv.promotion().map(Kind::from),
    }
}

/// One verifiable statement about a move. The first group are consequences for the player who
/// moved; the second group are things the move achieves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "fact", rename_all = "snake_case")]
pub enum Fact {
    /// The opponent can force checkmate in `moves` after the move.
    MateAllowed { moves: u32 },
    /// The player had a forced mate in `moves` and gave it up.
    MateMissed { moves: u32 },
    /// Along the engine's line the move leaves the player `points` of material worse off than the
    /// best move would.
    MaterialLost { points: i32 },
    /// After the opponent's best reply, these pieces are attacked and would be lost.
    Loose { reply: String, pieces: Vec<Spot> },
    /// Along the engine's line the player's king has to step out of check, to `square`.
    ForcedKingMove {
        line: Vec<String>,
        square: String,
        loses_castling: bool,
    },
    /// The opponent's best reply forks several of the player's pieces.
    AllowsFork {
        reply: String,
        attacker: Kind,
        targets: Vec<Spot>,
    },
    /// The opponent's best reply pins one of the player's pieces to the king.
    AllowsPin {
        reply: String,
        slider: Kind,
        pinned: Spot,
    },
    /// The move leads to a forced checkmate in `moves`.
    ForcesMate { moves: u32 },
    /// Along the engine's line the move wins `points` of material.
    WinsMaterial { points: i32 },
    /// The move forks several enemy pieces.
    Forks { attacker: Kind, targets: Vec<Spot> },
    /// The move pins an enemy piece to the king.
    Pins { slider: Kind, pinned: Spot },
}

impl Fact {
    /// True for facts that explain why a move was bad.
    pub fn is_consequence(&self) -> bool {
        matches!(
            self,
            Fact::MateAllowed { .. }
                | Fact::MateMissed { .. }
                | Fact::MaterialLost { .. }
                | Fact::Loose { .. }
                | Fact::ForcedKingMove { .. }
                | Fact::AllowsFork { .. }
                | Fact::AllowsPin { .. }
        )
    }

    /// How much the fact matters to a reader; facts are listed most important first.
    pub fn weight(&self) -> i32 {
        match self {
            Fact::MateAllowed { .. } => 100,
            Fact::MateMissed { .. } => 95,
            Fact::ForcesMate { .. } => 90,
            Fact::MaterialLost { points } => 70 + (*points).min(20),
            Fact::WinsMaterial { points } => 60 + (*points).min(20),
            Fact::AllowsFork { .. } => 68,
            Fact::Forks { .. } => 65,
            Fact::Loose { .. } => 64,
            Fact::ForcedKingMove {
                loses_castling: true,
                ..
            } => 60,
            Fact::ForcedKingMove { .. } => 50,
            Fact::AllowsPin { .. } => 55,
            Fact::Pins { .. } => 45,
        }
    }
}

/// Everything a renderer may say about one move.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Digest {
    pub ply: usize,
    pub mover: Side,
    pub san: String,
    /// The engine-derived class, as given; never re-judged here.
    pub class: MoveClass,
    pub loss: f64,
    pub eval_before: Eval,
    pub eval_after: Eval,
    pub played_is_best: bool,
    pub best_san: Option<String>,
    pub played: MoveFacts,
    pub best: Option<MoveFacts>,
    /// The played move followed by the engine's expected replies, in SAN.
    pub played_line: Vec<String>,
    /// The engine's best line, in SAN (empty when the played move was the best one).
    pub best_line: Vec<String>,
    /// Material, in pawns, from the mover's point of view.
    pub material_now: i32,
    pub material_played: i32,
    pub material_best: Option<i32>,
    /// Most important first.
    pub facts: Vec<Fact>,
}

/// What `digest` needs: the position before the move, the engine's verdict on the move, and the
/// engine's expected reply line from the position after it (empty if unknown).
#[derive(Debug, Clone, Copy)]
pub struct CommentaryInput<'a> {
    pub fen_before: &'a str,
    pub review: &'a MoveReview,
    pub reply_pv: &'a [String],
}

/// A line of moves replayed on a board, stopping at the first one that is not legal.
struct Line {
    moves: Vec<Move>,
    sans: Vec<String>,
    /// `positions[i]` is the position before `moves[i]`; the last is the position at the end.
    positions: Vec<Chess>,
}

impl Line {
    fn play(start: &Chess, ucis: &[String], limit: usize) -> Line {
        let mut line = Line {
            moves: Vec::new(),
            sans: Vec::new(),
            positions: vec![start.clone()],
        };
        for text in ucis.iter().take(limit) {
            let position = line.positions.last().expect("a line has a start").clone();
            let Some(mv) = parse_move(&position, text) else {
                break;
            };
            line.sans
                .push(SanPlus::from_move(position.clone(), mv).to_string());
            let mut next = position;
            next.play_unchecked(mv);
            line.moves.push(mv);
            line.positions.push(next);
        }
        line
    }

    /// The position after `plies` moves, or at the end of the line if it is shorter.
    fn at(&self, plies: usize) -> &Chess {
        &self.positions[plies.min(self.moves.len())]
    }
}

fn parse_move(position: &Chess, text: &str) -> Option<Move> {
    UciMove::from_ascii(text.as_bytes())
        .ok()?
        .to_move(position)
        .ok()
}

fn spots(targets: &[(Role, shakmaty::Square)]) -> Vec<Spot> {
    targets.iter().map(|&(role, sq)| spot(role, sq)).collect()
}

/// Works out the facts about the move in `input`, or `None` if the position or the move cannot
/// be read.
pub fn digest(input: &CommentaryInput<'_>) -> Option<Digest> {
    let review = input.review;
    let start: Chess = input
        .fen_before
        .parse::<Fen>()
        .ok()?
        .into_position(CastlingMode::Standard)
        .ok()?;
    let mover: Color = start.turn();
    let side = Side::from(mover);
    let played_move = parse_move(&start, &review.uci)?;
    let played_is_best = review.best_uci.as_deref() == Some(review.uci.as_str());

    let mut played_ucis = vec![review.uci.clone()];
    played_ucis.extend(input.reply_pv.iter().cloned());
    let played_line = Line::play(&start, &played_ucis, MAX_PLIES);
    let best_line = (!played_is_best && !review.best_pv.is_empty())
        .then(|| Line::play(&start, &review.best_pv, MAX_PLIES));

    let mut horizon = MAX_PLIES
        .min(played_line.moves.len())
        .min(best_line.as_ref().map_or(MAX_PLIES, |l| l.moves.len()));
    if horizon > 1 && horizon % 2 == 1 {
        horizon -= 1;
    }
    let material_now = balance(&start, mover);
    let material_played = balance(played_line.at(horizon), mover);
    let material_best = best_line
        .as_ref()
        .map(|line| balance(line.at(horizon), mover));

    let mut facts = Vec::new();

    match review.eval_after {
        Eval::Mate(n) if review.eval_after.is_mate_against(side) => {
            facts.push(Fact::MateAllowed {
                moves: n.unsigned_abs(),
            });
        }
        Eval::Mate(n) if review.eval_after.is_mate_for(side) => {
            facts.push(Fact::ForcesMate {
                moves: n.unsigned_abs(),
            });
        }
        _ => {}
    }
    if let Eval::Mate(n) = review.eval_before
        && review.eval_before.is_mate_for(side)
        && !review.eval_after.is_mate_for(side)
    {
        facts.push(Fact::MateMissed {
            moves: n.unsigned_abs(),
        });
    }

    // Once a forced mate is on the board, material along the line says nothing useful ("wins a
    // queen" in a line that ends with the mover mated).
    let line_ends_in_mate = |line: &Line| line.at(line.moves.len()).is_checkmate();
    let mate_in_play = matches!(review.eval_after, Eval::Mate(_) | Eval::Checkmate(_))
        || line_ends_in_mate(&played_line)
        || best_line.as_ref().is_some_and(line_ends_in_mate);
    if !mate_in_play {
        if let Some(best) = material_best
            && best - material_played >= 1
        {
            facts.push(Fact::MaterialLost {
                points: best - material_played,
            });
        }
        if material_played - material_now >= 1 {
            facts.push(Fact::WinsMaterial {
                points: material_played - material_now,
            });
        }
    }

    if played_line.moves.len() >= 2 {
        let reply = played_line.sans[1].clone();
        let after_reply = played_line.at(2);
        // Once the reply ends the game there is nothing left to defend.
        let loose = if after_reply.is_game_over() {
            Vec::new()
        } else {
            exchange::loose_pieces(after_reply, mover)
        };
        if !loose.is_empty() {
            let shown: Vec<_> = loose.into_iter().take(2).collect();
            facts.push(Fact::Loose {
                reply: reply.clone(),
                pieces: spots(&shown),
            });
        }
        let reply_from = &played_line.positions[1];
        if let Some(fork) = motifs::fork(reply_from, played_line.moves[1]) {
            facts.push(Fact::AllowsFork {
                reply: reply.clone(),
                attacker: fork.attacker.into(),
                targets: spots(&fork.targets),
            });
        }
        if let Some(pin) = motifs::pin(reply_from, played_line.moves[1]) {
            facts.push(Fact::AllowsPin {
                reply,
                slider: pin.slider.into(),
                pinned: spot(pin.pinned.0, pin.pinned.1),
            });
        }
    }
    if let Some(fact) = king::forced_king_move(&played_line, mover) {
        facts.push(fact);
    }
    if let Some(fork) = motifs::fork(&start, played_move) {
        facts.push(Fact::Forks {
            attacker: fork.attacker.into(),
            targets: spots(&fork.targets),
        });
    }
    if let Some(pin) = motifs::pin(&start, played_move) {
        facts.push(Fact::Pins {
            slider: pin.slider.into(),
            pinned: spot(pin.pinned.0, pin.pinned.1),
        });
    }
    facts.sort_by_key(|fact| std::cmp::Reverse(fact.weight()));

    let best_move = best_line
        .as_ref()
        .and_then(|line| line.moves.first().copied());
    Some(Digest {
        ply: review.ply,
        mover: side,
        san: review.san.clone(),
        class: review.class,
        loss: review.loss,
        eval_before: review.eval_before,
        eval_after: review.eval_after,
        played_is_best,
        best_san: review.best_san.clone(),
        played: move_facts(&start, played_move),
        best: best_move.map(|mv| move_facts(&start, mv)),
        played_line: played_line.sans.clone(),
        best_line: best_line.map(|line| line.sans).unwrap_or_default(),
        material_now,
        material_played,
        material_best,
        facts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 6...Nf6 in Morphy's Opera Game, with the lines Stockfish gave at depth 18.
    const BEFORE_NF6: &str = "rn1qkbnr/ppp2ppp/8/4p3/2B1P3/5Q2/PPP2PPP/RNB1K2R b KQkq - 1 6";

    fn strings(moves: &[&str]) -> Vec<String> {
        moves.iter().map(|m| m.to_string()).collect()
    }

    fn nf6_review() -> MoveReview {
        MoveReview {
            ply: 12,
            move_number: 6,
            side: Side::Black,
            san: "Nf6".into(),
            uci: "g8f6".into(),
            class: MoveClass::Mistake,
            eval_before: Eval::Cp(146),
            eval_after: Eval::Cp(327),
            best_uci: Some("d8f6".into()),
            best_san: Some("Qf6".into()),
            best_pv: strings(&["d8f6", "f3b3", "b8d7", "b3b7", "a8b8", "b7a6"]),
            loss: 13.8,
            accuracy: 40.0,
            critical: true,
            commentary: None,
        }
    }

    fn nf6_reply() -> Vec<String> {
        strings(&["f3b3", "f8c5", "c4f7", "e8e7", "f7c4", "h8f8"])
    }

    fn nf6_digest() -> Digest {
        let review = nf6_review();
        let reply = nf6_reply();
        digest(&CommentaryInput {
            fen_before: BEFORE_NF6,
            review: &review,
            reply_pv: &reply,
        })
        .expect("a digest")
    }

    #[test]
    fn the_digest_names_the_move_and_the_lines_in_san() {
        let d = nf6_digest();
        assert_eq!(d.mover, Side::Black);
        assert_eq!(d.san, "Nf6");
        assert_eq!(d.best_san.as_deref(), Some("Qf6"));
        assert_eq!(
            d.played_line,
            strings(&["Nf6", "Qb3", "Bc5", "Bxf7+", "Ke7", "Bc4"])
        );
        assert_eq!(
            d.best_line,
            strings(&["Qf6", "Qb3", "Nd7", "Qxb7", "Rb8", "Qa6"])
        );
        assert!(!d.played_is_best);
    }

    #[test]
    fn material_is_compared_at_the_same_point_of_both_lines() {
        // Both lines end with White a pawn up, which is exactly why material alone cannot
        // explain this mistake.
        let d = nf6_digest();
        assert_eq!(d.material_now, 0);
        assert_eq!(d.material_played, -1);
        assert_eq!(d.material_best, Some(-1));
        assert!(
            !d.facts
                .iter()
                .any(|f| matches!(f, Fact::MaterialLost { .. }))
        );
    }

    #[test]
    fn the_digest_finds_the_pieces_left_short_of_protection_and_the_driven_king() {
        let d = nf6_digest();
        assert!(d.facts.contains(&Fact::Loose {
            reply: "Qb3".into(),
            pieces: vec![
                Spot {
                    kind: Kind::Pawn,
                    square: "b7".into()
                },
                Spot {
                    kind: Kind::Pawn,
                    square: "f7".into()
                },
            ],
        }));
        assert!(d.facts.contains(&Fact::ForcedKingMove {
            line: strings(&["Qb3", "Bc5", "Bxf7+", "Ke7"]),
            square: "e7".into(),
            loses_castling: true,
        }));
    }

    #[test]
    fn facts_are_ranked_most_important_first() {
        let d = nf6_digest();
        let weights: Vec<i32> = d.facts.iter().map(Fact::weight).collect();
        let mut sorted = weights.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(weights, sorted);
        assert!(d.facts.len() >= 2);
    }

    #[test]
    fn the_move_itself_is_described() {
        let d = nf6_digest();
        assert_eq!(
            d.played,
            MoveFacts {
                kind: Kind::Knight,
                captures: None,
                check: false,
                mate: false,
                castles: false,
                promotes: None
            }
        );
        assert_eq!(d.best.as_ref().map(|b| b.kind), Some(Kind::Queen));
    }

    #[test]
    fn an_unreadable_position_or_move_gives_no_digest() {
        let review = nf6_review();
        assert!(
            digest(&CommentaryInput {
                fen_before: "not a fen",
                review: &review,
                reply_pv: &[]
            })
            .is_none()
        );
        let mut illegal = nf6_review();
        illegal.uci = "e2e4".into();
        assert!(
            digest(&CommentaryInput {
                fen_before: BEFORE_NF6,
                review: &illegal,
                reply_pv: &[]
            })
            .is_none()
        );
    }

    #[test]
    fn a_reply_line_that_stops_being_legal_just_ends() {
        let review = nf6_review();
        let reply = strings(&["f3b3", "a1a8"]);
        let d = digest(&CommentaryInput {
            fen_before: BEFORE_NF6,
            review: &review,
            reply_pv: &reply,
        })
        .expect("a digest");
        assert_eq!(d.played_line, strings(&["Nf6", "Qb3"]));
    }

    #[test]
    fn a_move_with_no_known_reply_has_no_reply_facts() {
        let review = nf6_review();
        let d = digest(&CommentaryInput {
            fen_before: BEFORE_NF6,
            review: &review,
            reply_pv: &[],
        })
        .expect("a digest");
        assert_eq!(d.played_line, strings(&["Nf6"]));
        assert!(
            !d.facts
                .iter()
                .any(|f| matches!(f, Fact::Loose { .. } | Fact::ForcedKingMove { .. }))
        );
    }

    #[test]
    fn dropping_a_forced_mate_and_allowing_one_are_facts() {
        // White to move with a back-rank mate in one (Ra8#); playing Kf1 instead.
        let fen = "6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1";
        let mut review = nf6_review();
        review.side = Side::White;
        review.san = "Kf1".into();
        review.uci = "g1f1".into();
        review.best_uci = Some("a1a8".into());
        review.best_san = Some("Ra8#".into());
        review.best_pv = strings(&["a1a8"]);
        review.eval_before = Eval::Mate(1);
        review.eval_after = Eval::Cp(0);
        let d = digest(&CommentaryInput {
            fen_before: fen,
            review: &review,
            reply_pv: &[],
        })
        .expect("a digest");
        assert!(d.facts.contains(&Fact::MateMissed { moves: 1 }));
        assert!(d.best.as_ref().is_some_and(|b| b.mate));

        review.eval_before = Eval::Cp(0);
        review.eval_after = Eval::Mate(-2);
        let d = digest(&CommentaryInput {
            fen_before: fen,
            review: &review,
            reply_pv: &[],
        })
        .expect("a digest");
        assert!(d.facts.contains(&Fact::MateAllowed { moves: 2 }));
    }

    #[test]
    fn a_reply_that_forks_or_pins_is_a_fact() {
        let mut review = nf6_review();
        review.san = "Kd7".into();
        review.uci = "e8d7".into();
        review.best_uci = Some("a8a7".into());
        review.best_san = Some("Ra7".into());
        review.best_pv = strings(&["a8a7"]);
        // The knight on d5 forks the king on d7 and the rook on a8 with Nb6+.
        let fork_reply = strings(&["d5b6"]);
        let d = digest(&CommentaryInput {
            fen_before: "r3k3/8/8/3N4/8/8/8/4K3 b - - 0 1",
            review: &review,
            reply_pv: &fork_reply,
        })
        .expect("a digest");
        let spot = |kind, square: &str| Spot {
            kind,
            square: square.into(),
        };
        assert!(d.facts.contains(&Fact::AllowsFork {
            reply: "Nb6+".into(),
            attacker: Kind::Knight,
            targets: vec![spot(Kind::King, "d7"), spot(Kind::Rook, "a8")],
        }));

        // Re1 pins the knight on e7 to the king on e8.
        review.san = "a6".into();
        review.uci = "a7a6".into();
        let pin_reply = strings(&["f1e1"]);
        let d = digest(&CommentaryInput {
            fen_before: "4k3/p3n3/8/8/8/8/8/5RK1 b - - 0 1",
            review: &review,
            reply_pv: &pin_reply,
        })
        .expect("a digest");
        assert!(d.facts.contains(&Fact::AllowsPin {
            reply: "Re1".into(),
            slider: Kind::Rook,
            pinned: spot(Kind::Knight, "e7"),
        }));
    }

    #[test]
    fn a_move_that_forks_or_pins_is_a_fact_about_the_move() {
        let mut review = nf6_review();
        review.side = Side::White;
        review.class = MoveClass::Best;
        review.san = "Nc7+".into();
        review.uci = "d5c7".into();
        review.best_uci = Some("d5c7".into());
        review.best_san = Some("Nc7+".into());
        review.best_pv = strings(&["d5c7"]);
        let d = digest(&CommentaryInput {
            fen_before: "r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1",
            review: &review,
            reply_pv: &[],
        })
        .expect("a digest");
        assert!(d.facts.iter().any(|f| matches!(
            f,
            Fact::Forks {
                attacker: Kind::Knight,
                targets
            } if targets.len() == 2
        )));

        review.san = "Re1".into();
        review.uci = "f1e1".into();
        review.best_uci = Some("f1e1".into());
        review.best_san = Some("Re1".into());
        review.best_pv = strings(&["f1e1"]);
        let d = digest(&CommentaryInput {
            fen_before: "4k3/4n3/8/8/8/8/8/5RK1 w - - 0 1",
            review: &review,
            reply_pv: &[],
        })
        .expect("a digest");
        assert!(d.facts.iter().any(|f| matches!(
            f,
            Fact::Pins {
                slider: Kind::Rook,
                ..
            }
        )));
    }

    #[test]
    fn castling_promotion_and_en_passant_are_described_without_trouble() {
        let mut review = nf6_review();
        review.side = Side::White;
        review.class = MoveClass::Best;
        let mut describe = |fen: &str, uci: &str, san: &str| {
            review.san = san.into();
            review.uci = uci.into();
            review.best_uci = Some(uci.into());
            review.best_san = Some(san.into());
            review.best_pv = strings(&[uci]);
            digest(&CommentaryInput {
                fen_before: fen,
                review: &review,
                reply_pv: &[],
            })
            .expect("a digest")
            .played
        };
        let castle = describe("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", "e1g1", "O-O");
        assert!(castle.castles && castle.kind == Kind::King);
        let promotion = describe("7k/P7/8/8/8/8/8/K7 w - - 0 1", "a7a8q", "a8=Q+");
        assert_eq!(promotion.promotes, Some(Kind::Queen));
        assert!(promotion.check);
        let en_passant = describe("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2", "e5d6", "exd6");
        assert_eq!(en_passant.captures, Some(Kind::Pawn));
    }

    #[test]
    fn nothing_is_loose_after_a_reply_that_ends_the_game() {
        // 1. f3 e5 2. g4 Qh4#: the g4 pawn is "attacked" but the game is over.
        let fen = "rnbqkbnr/pppp1ppp/8/4p3/8/5P2/PPPPP1PP/RNBQKBNR w KQkq - 0 2";
        let mut review = nf6_review();
        review.side = Side::White;
        review.san = "g4".into();
        review.uci = "g2g4".into();
        review.best_uci = Some("d2d4".into());
        review.best_san = Some("d4".into());
        review.best_pv = strings(&["d2d4"]);
        review.eval_before = Eval::Cp(-60);
        review.eval_after = Eval::Mate(-1);
        let reply = strings(&["d8h4"]);
        let d = digest(&CommentaryInput {
            fen_before: fen,
            review: &review,
            reply_pv: &reply,
        })
        .expect("a digest");
        assert_eq!(d.facts, vec![Fact::MateAllowed { moves: 1 }]);
    }

    #[test]
    fn material_is_not_mentioned_when_a_forced_mate_is_in_play() {
        // White takes a free knight but the engine sees Black mating anyway.
        let fen = "3k4/8/8/3n4/8/8/8/3QK3 w - - 0 1";
        let mut review = nf6_review();
        review.side = Side::White;
        review.san = "Qxd5+".into();
        review.uci = "d1d5".into();
        review.best_uci = Some("d1d5".into());
        review.best_pv = strings(&["d1d5"]);
        review.eval_after = Eval::Mate(-3);
        let d = digest(&CommentaryInput {
            fen_before: fen,
            review: &review,
            reply_pv: &[],
        })
        .expect("a digest");
        assert!(
            d.facts
                .iter()
                .all(|f| !matches!(f, Fact::WinsMaterial { .. } | Fact::MaterialLost { .. }))
        );
        assert!(d.facts.contains(&Fact::MateAllowed { moves: 3 }));
    }

    #[test]
    fn material_is_not_mentioned_when_the_line_itself_ends_in_checkmate() {
        // 1. f3 e5 2. g4 Qh4#: the engine's evaluation may be a plain score at low depth, but the
        // line ends in mate.
        let fen = "rnbqkbnr/pppp1ppp/8/4p3/8/5P2/PPPPP1PP/RNBQKBNR w KQkq - 0 2";
        let mut review = nf6_review();
        review.side = Side::White;
        review.san = "g4".into();
        review.uci = "g2g4".into();
        review.best_uci = Some("d2d4".into());
        review.best_san = Some("d4".into());
        review.best_pv = strings(&["d2d4"]);
        review.eval_after = Eval::Cp(-900);
        let reply = strings(&["d8h4"]);
        let d = digest(&CommentaryInput {
            fen_before: fen,
            review: &review,
            reply_pv: &reply,
        })
        .expect("a digest");
        assert!(
            d.facts
                .iter()
                .all(|f| !matches!(f, Fact::WinsMaterial { .. } | Fact::MaterialLost { .. }))
        );
    }

    #[test]
    fn winning_material_along_the_line_is_a_fact() {
        // White takes a free knight.
        let fen = "3k4/8/8/3n4/8/8/8/3QK3 w - - 0 1";
        let mut review = nf6_review();
        review.side = Side::White;
        review.san = "Qxd5+".into();
        review.uci = "d1d5".into();
        review.class = MoveClass::Best;
        review.best_uci = Some("d1d5".into());
        review.best_san = Some("Qxd5+".into());
        review.best_pv = strings(&["d1d5"]);
        let d = digest(&CommentaryInput {
            fen_before: fen,
            review: &review,
            reply_pv: &[],
        })
        .expect("a digest");
        assert_eq!(d.played.captures, Some(Kind::Knight));
        assert!(d.played.check);
        assert!(d.facts.contains(&Fact::WinsMaterial { points: 3 }));
    }
}
