//! King safety along an engine line.

use shakmaty::{Color, Position, Role};

use super::{Fact, Line};

/// If `mover`'s king has to step out of a check later in `line` (the opponent's check, not one
/// that was already on the board before the first move), the sequence of moves from the
/// opponent's reply up to and including that king move, where the king ended up, and whether
/// `mover` could castle before and cannot afterwards. The first move of `line` is `mover`'s own
/// move and is not counted.
pub(super) fn forced_king_move(line: &Line, mover: Color) -> Option<Fact> {
    let could_castle = line.positions.first()?.castles().has_color(mover);
    for (i, mv) in line.moves.iter().enumerate().skip(1) {
        let before = &line.positions[i];
        if before.turn() == mover && before.is_check() && mv.role() == Role::King && !mv.is_castle()
        {
            let after = &line.positions[i + 1];
            return Some(Fact::ForcedKingMove {
                line: line.sans[1..=i].to_vec(),
                square: mv.to().to_string(),
                loses_castling: could_castle && !after.castles().has_color(mover),
            });
        }
    }
    None
}
