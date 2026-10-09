//! Tactical motifs created by a single move: forks and pins.
//!
//! Both look at the position *after* the move and ask what the moved piece now does. They are
//! deliberately conservative: a fork needs a moved piece that cannot simply be taken, and a pin
//! needs a piece worth at least a minor piece pinned against the king.

use shakmaty::attacks::attacks;
use shakmaty::{Chess, Move, Piece, Position, Role, Square};

use super::exchange::exchange_gain;

/// The piece that forked and the pieces it attacks at once (the king counts).
pub(super) struct Fork {
    pub attacker: Role,
    pub targets: Vec<(Role, Square)>,
}

/// The sliding piece that pinned, and the piece it pins against the king.
pub(super) struct Pin {
    pub slider: Role,
    pub pinned: (Role, Square),
}

fn worth_at_least_a_minor(role: Role) -> bool {
    matches!(role, Role::Knight | Role::Bishop | Role::Rook | Role::Queen)
}

/// Did `mv` (played from `before`) attack two or more enemy pieces at once with a piece that
/// cannot be taken at a profit? Attacked pieces count when they are the king, or when taking them
/// would win material.
pub(super) fn fork(before: &Chess, mv: Move) -> Option<Fork> {
    if mv.is_castle() {
        return None;
    }
    let mover = before.turn();
    let mut after = before.clone();
    after.play_unchecked(mv);
    let board = after.board();
    let to = mv.to();
    let role = board.role_at(to)?;
    if exchange_gain(board, to, mover.other()) > 0 {
        return None;
    }
    let reach = attacks(to, Piece { color: mover, role }, board.occupied());
    let targets: Vec<(Role, Square)> = (reach & board.by_color(mover.other()))
        .into_iter()
        .filter_map(|square| {
            let target = board.role_at(square)?;
            (target == Role::King || exchange_gain(board, square, mover) > 0)
                .then_some((target, square))
        })
        .collect();
    (targets.len() >= 2).then_some(Fork {
        attacker: role,
        targets,
    })
}

/// Did `mv` (played from `before`) leave a bishop, rook or queen pinning a piece worth at least
/// a minor piece against the enemy king? A move that gives check is not a pin.
pub(super) fn pin(before: &Chess, mv: Move) -> Option<Pin> {
    if mv.is_castle() {
        return None;
    }
    let mover = before.turn();
    let mut after = before.clone();
    after.play_unchecked(mv);
    let board = after.board();
    let to = mv.to();
    let role = board.role_at(to)?;
    if !matches!(role, Role::Bishop | Role::Rook | Role::Queen) {
        return None;
    }
    let king = board.king_of(mover.other())?;
    let slider = Piece { color: mover, role };
    let occupied = board.occupied();
    let reach = attacks(to, slider, occupied);
    if reach.contains(king) {
        return None;
    }
    (reach & board.by_color(mover.other()))
        .into_iter()
        .find_map(|square| {
            let pinned = board.role_at(square)?;
            let revealed = attacks(to, slider, occupied.without(square));
            (worth_at_least_a_minor(pinned) && revealed.contains(king)).then_some(Pin {
                slider: role,
                pinned: (pinned, square),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::CastlingMode;
    use shakmaty::fen::Fen;
    use shakmaty::uci::UciMove;

    fn position(fen: &str) -> Chess {
        fen.parse::<Fen>()
            .unwrap()
            .into_position(CastlingMode::Standard)
            .unwrap()
    }

    fn mv(pos: &Chess, uci: &str) -> Move {
        UciMove::from_ascii(uci.as_bytes())
            .unwrap()
            .to_move(pos)
            .unwrap()
    }

    fn square(name: &str) -> Square {
        name.parse().unwrap()
    }

    #[test]
    fn a_knight_forking_king_and_rook_is_a_fork() {
        let pos = position("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1");
        let found = fork(&pos, mv(&pos, "d5c7")).expect("a fork");
        assert_eq!(found.attacker, Role::Knight);
        assert_eq!(
            found.targets,
            vec![(Role::Rook, square("a8")), (Role::King, square("e8"))]
        );
    }

    #[test]
    fn attacking_one_piece_is_not_a_fork() {
        let pos = position("r3k3/8/8/3N4/8/8/8/4K3 w - - 0 1");
        assert!(fork(&pos, mv(&pos, "d5b6")).is_none());
    }

    #[test]
    fn a_forking_piece_that_can_be_taken_is_not_a_fork() {
        // The bishop on d6 takes the knight on c7.
        let pos = position("r3k3/8/3b4/3N4/8/8/8/4K3 w - - 0 1");
        assert!(fork(&pos, mv(&pos, "d5c7")).is_none());
    }

    #[test]
    fn a_rook_stepping_behind_a_knight_pins_it_to_the_king() {
        let pos = position("4k3/4n3/8/8/8/8/8/5RK1 w - - 0 1");
        let found = pin(&pos, mv(&pos, "f1e1")).expect("a pin");
        assert_eq!(found.slider, Role::Rook);
        assert_eq!(found.pinned, (Role::Knight, square("e7")));
    }

    #[test]
    fn a_check_is_not_a_pin_and_a_pinned_pawn_is_too_small_to_mention() {
        let check = position("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
        assert!(pin(&check, mv(&check, "a1a8")).is_none());
        let pawn = position("4k3/4p3/8/8/8/8/8/5RK1 w - - 0 1");
        assert!(pin(&pawn, mv(&pawn, "f1e1")).is_none());
    }
}
