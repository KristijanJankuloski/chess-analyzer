//! Exchange evaluation: what a side gains by starting captures on one square.
//!
//! This is the classic "static exchange evaluation". It is aware of batteries (a queen standing
//! behind a bishop joins the attack once the bishop has captured), because the attackers of the
//! square are looked up again after every capture, with the capturing piece removed. It does not
//! know about absolute pins, promotions or en passant, so it is an approximation, which is fine
//! for deciding which pieces to mention.

use shakmaty::{Bitboard, Board, Chess, Color, Position, Role, Square};

/// A piece's worth in the exchange. The king is worth more than everything else together, so a
/// side never "wins" a capture by losing its king.
fn exchange_value(role: Role) -> i32 {
    match role {
        Role::Pawn => 1,
        Role::Knight | Role::Bishop => 3,
        Role::Rook => 5,
        Role::Queen => 9,
        Role::King => 100,
    }
}

/// The cheapest piece of `side` that attacks `target` when only `occupied` squares hold pieces.
fn cheapest_attacker(
    board: &Board,
    target: Square,
    side: Color,
    occupied: Bitboard,
) -> Option<(Square, Role)> {
    let attackers = board.attacks_to(target, side, occupied) & occupied;
    [
        Role::Pawn,
        Role::Knight,
        Role::Bishop,
        Role::Rook,
        Role::Queen,
        Role::King,
    ]
    .into_iter()
    .find_map(|role| {
        (attackers & board.by_role(role))
            .first()
            .map(|sq| (sq, role))
    })
}

/// What `attacker` nets, in pawns, by capturing the piece on `target` and then playing out the
/// best captures and recaptures on that square. Zero if there is no piece of the other side
/// there, or if capturing it would lose material.
pub(super) fn exchange_gain(board: &Board, target: Square, attacker: Color) -> i32 {
    let Some(victim) = board.role_at(target) else {
        return 0;
    };
    if board.color_at(target) == Some(attacker) {
        return 0;
    }
    let mut occupied = board.occupied();
    let Some((mut from, mut role)) = cheapest_attacker(board, target, attacker, occupied) else {
        return 0;
    };
    let mut side = attacker;
    let mut gain = [0i32; 40];
    gain[0] = exchange_value(victim);
    let mut depth = 0;
    loop {
        depth += 1;
        // What the other side wins if this piece is recaptured.
        gain[depth] = exchange_value(role) - gain[depth - 1];
        if depth + 1 >= gain.len() {
            break;
        }
        occupied.discard(from);
        side = side.other();
        match cheapest_attacker(board, target, side, occupied) {
            Some((next_from, next_role)) => {
                from = next_from;
                role = next_role;
            }
            None => break,
        }
    }
    while depth > 1 {
        depth -= 1;
        gain[depth - 1] = -(-gain[depth - 1]).max(gain[depth]);
    }
    gain[0].max(0)
}

/// The pieces of `color` (never the king) that the other side would win material by capturing,
/// most valuable first, ties broken by square.
pub(super) fn loose_pieces(position: &Chess, color: Color) -> Vec<(Role, Square)> {
    let board = position.board();
    let mut loose: Vec<(Role, Square)> = board
        .by_color(color)
        .into_iter()
        .filter_map(|square| {
            let role = board.role_at(square)?;
            (role != Role::King && exchange_gain(board, square, color.other()) > 0)
                .then_some((role, square))
        })
        .collect();
    loose.sort_by_key(|&(role, square)| (std::cmp::Reverse(exchange_value(role)), square));
    loose
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::fen::Fen;
    use shakmaty::{CastlingMode, Chess};

    fn position(fen: &str) -> Chess {
        fen.parse::<Fen>()
            .unwrap()
            .into_position(CastlingMode::Standard)
            .unwrap()
    }

    fn square(name: &str) -> Square {
        name.parse().unwrap()
    }

    #[test]
    fn an_undefended_piece_is_won_outright() {
        let pos = position("4k3/8/8/3n4/8/8/8/3QK3 b - - 0 1");
        assert_eq!(exchange_gain(pos.board(), square("d5"), Color::White), 3);
    }

    #[test]
    fn a_defended_piece_is_not_worth_taking_with_a_queen() {
        let pos = position("4k3/8/4p3/3n4/8/8/8/3QK3 w - - 0 1");
        assert_eq!(exchange_gain(pos.board(), square("d5"), Color::White), 0);
    }

    #[test]
    fn a_cheaper_attacker_wins_the_exchange_even_when_the_target_is_defended() {
        // The pawn on c4 takes the defended knight on d5 and is recaptured: 3 - 1.
        let pos = position("4k3/8/4p3/3n4/2P5/8/8/4K3 w - - 0 1");
        assert_eq!(exchange_gain(pos.board(), square("d5"), Color::White), 2);
    }

    #[test]
    fn a_piece_standing_behind_the_attacker_joins_the_attack() {
        // After 6...Nf6 7.Qb3 the f7 pawn is attacked by Bc4 with the queen behind it on the
        // same diagonal, and defended only by the king. Counting attackers directly sees one
        // attacker; the exchange sees Bxf7+ Kxf7?? Qxf7 and so Black cannot recapture.
        let pos = position("rn1qkb1r/ppp2ppp/5n2/4p3/2B1P3/1Q6/PPP2PPP/RNB1K2R b KQkq - 3 7");
        assert_eq!(exchange_gain(pos.board(), square("f7"), Color::White), 1);
        // Without the queen behind the bishop the king can simply take back.
        let alone = position("rn1qkb1r/ppp2ppp/5n2/4p3/2B1P3/5Q2/PPP2PPP/RNB1K2R w KQkq - 2 7");
        assert_eq!(exchange_gain(alone.board(), square("f7"), Color::White), 0);
    }

    #[test]
    fn loose_pieces_lists_the_most_valuable_first() {
        let pos = position("rn1qkb1r/ppp2ppp/5n2/4p3/2B1P3/1Q6/PPP2PPP/RNB1K2R b KQkq - 3 7");
        let loose = loose_pieces(&pos, Color::Black);
        assert_eq!(
            loose,
            vec![(Role::Pawn, square("b7")), (Role::Pawn, square("f7"))]
        );
    }

    #[test]
    fn nothing_is_loose_in_a_quiet_position() {
        let pos = position("rn1qkb1r/ppp2ppp/5n2/4p3/2B1P3/5Q2/PPP2PPP/RNB1K2R w KQkq - 2 7");
        assert!(loose_pieces(&pos, Color::Black).is_empty());
    }

    #[test]
    fn the_king_is_never_listed_and_an_empty_square_gains_nothing() {
        let pos = position("4k3/8/8/8/8/8/4q3/4K3 w - - 0 1");
        assert!(loose_pieces(&pos, Color::White).is_empty());
        assert_eq!(exchange_gain(pos.board(), square("a1"), Color::White), 0);
    }
}
