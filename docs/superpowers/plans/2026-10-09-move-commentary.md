# Move Commentary (Milestone 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every move in a finished review and in a live game gets a short, correct, plain-language explanation written from Stockfish's own lines, with no language model and no network.

**Architecture:** `core::facts` replays the played move and the engine's lines on a `shakmaty` board and returns a ranked, typed `Digest`. `core::commentary` turns a digest into two or three sentences. `review::review_move`, the one function that classifies a move for both a finished review and the live session, stores the text in a new `MoveReview.commentary` field, so no new command, event or setting is needed. The UI shows that field and falls back to the old template sentence when there is none.

**Tech Stack:** Rust 2024 (1.88), `shakmaty` 0.30.2, `serde`, `ts-rs` (all already dependencies; nothing new is added); React, TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-09-move-commentary-design.md` (Milestone 1). Milestone 2, the optional local-LLM coach, is deliberately not part of this plan.

**How this plan was checked:** every Rust and TypeScript block below was compiled and run in a scratch worktree before it was written down, and is pasted from those files. With exactly this code, `cargo clippy --all-targets -- -D warnings` was clean and the suites passed (216 core unit tests, the golden and grounding tests, 9 CLI tests, 302 frontend tests). If a step's expected output differs from what you see, suspect a transcription slip before suspecting the design.

## Global Constraints

- Stockfish is the authority on chess; language is only a commentator. Neither the digest nor the renderer may calculate, judge a move, or contradict the engine's class or best move (`AGENTS.md`, spec "Intent").
- No new dependencies. Rust edition 2024, `rust-version` 1.88, `shakmaty` 0.30.2.
- Everything stays offline: no network access of any kind in this milestone.
- All wording lives in `crates/core/src/commentary.rs`, in English, and is deterministic (the same move always reads the same).
- Facts are typed (`enum Fact`), never strings; both the template renderer and the later LLM renderer match on them.
- `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` must pass (CI runs both).
- Generated TypeScript (`app/src/generated`) and the app fixtures (`app/src/fixtures`) are committed, and CI fails if `git diff` shows drift after the tests have run.
- Each commit leaves the Rust tests, the golden test and the frontend type check green.
- Run `cargo fmt --all` before every Rust commit (it also sorts imports, so the import lists given below may be reordered by it).

## Review Focus

The spec says what the software must do; these are inputs it will meet that no spec line names. Each line is pinned by the test named after it.

1. **An engine line that stops being legal, or no reply at all** (the last move; an empty principal variation). The line ends there and a sentence is still produced. Pinned by `a_reply_line_that_stops_being_legal_just_ends` and `a_move_with_no_known_reply_has_no_reply_facts` (Task 3).
2. **Castling, promotion and en passant as the played move.** Described without panicking or mislabelling. Pinned by `castling_promotion_and_en_passant_are_described_without_trouble` (Task 3).
3. **The move or its reply ends the game.** No "pieces are attacked" after a mating reply, no "wins a queen" in a line where the mover is mated, and a mating move says so. Pinned by `nothing_is_loose_after_a_reply_that_ends_the_game`, `material_is_not_mentioned_when_a_forced_mate_is_in_play` and `material_is_not_mentioned_when_the_line_itself_ends_in_checkmate` (Task 3), and `a_mating_move_says_so_whatever_its_class` (Task 4).
4. **Reviews saved before this feature** (no `commentary` key in the stored JSON). They load and get commentary. Pinned by `a_review_saved_before_commentary_existed_gets_it_when_loaded` (Task 5).
5. **Black to move, and a position or move that cannot be read.** The right colour is named; unreadable input gives no commentary and the UI falls back to the old sentence. Pinned by `colours_follow_the_mover` (Task 4), `an_unreadable_position_or_move_gives_no_digest` (Task 3) and "falls back to the short sentence" (Task 7).

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/core/src/facts.rs` | create | Types (`Kind`, `Spot`, `MoveFacts`, `Fact`, `Digest`, `CommentaryInput`) and `digest()`, which assembles the facts |
| `crates/core/src/facts/exchange.rs` | create | Static exchange evaluation; loose pieces |
| `crates/core/src/facts/motifs.rs` | create | Fork and pin detectors |
| `crates/core/src/facts/king.rs` | create | A king driven out of castling along a line |
| `crates/core/src/commentary.rs` | create | `Digest` to text (template renderer) |
| `crates/core/src/review.rs` | modify | `MoveReview.commentary`; `review_move` fills it; `digest_for`; `backfill_commentary` |
| `crates/core/src/store.rs` | modify | `get` fills in commentary for old reviews |
| `crates/core/src/lib.rs` | modify | Declare the two new modules |
| `crates/core/tests/golden.rs` | modify | Commentary in the snapshot; grounding test |
| `crates/cli/src/report.rs`, `main.rs` | modify | `--commentary` and `--facts` |
| `app/src/lib/commentary.ts` | modify | `commentaryFor` |
| `app/src/screens/ReviewScreen.tsx`, `LiveScreen.tsx` | modify | Show it |
| `data/fixtures/*.golden.txt`, `app/src/fixtures/*.stored.json`, `app/src/generated/MoveReview.ts` | regenerated | Generated by the test suite; committed |

Commands run from the repository root unless a step says `cd app`. The shell is Git Bash on Windows.

---

### Task 1: Exchange evaluation

**Files:**
- Create: `crates/core/src/facts.rs`
- Create: `crates/core/src/facts/exchange.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces (both `pub(super)`, used by Tasks 2 and 3):
  - `exchange_gain(board: &Board, target: Square, attacker: Color) -> i32`: what `attacker` nets, in pawns, by capturing the piece on `target` and playing out the best captures and recaptures; 0 if there is nothing to capture or capturing loses material.
  - `loose_pieces(position: &Chess, color: Color) -> Vec<(Role, Square)>`: the pieces of `color` (never the king) that the other side would win material by capturing, most valuable first, ties by square.

- [ ] **Step 1: Declare the module**

In `crates/core/src/lib.rs`, add `pub mod facts;` after `pub mod eval;`:

```rust
pub mod eval;
pub mod facts;
pub mod game;
```

Create `crates/core/src/facts.rs` (later tasks replace it):

```rust
//! Facts about one move, worked out from the engine's own lines. (Filled in by later tasks.)

mod exchange;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/core/src/facts/exchange.rs` containing only this test module. The battery case is the one that matters: after `6...Nf6 7.Qb3` the f7 pawn is attacked by `Bc4` with the queen behind it on the same diagonal, and a plain attacker count sees only one attacker.

```rust
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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p chess-analyzer-core --lib -- facts::exchange`
Expected: FAIL to compile, with errors such as ``cannot find function `exchange_gain` in this scope``.

- [ ] **Step 4: Write the implementation**

Put this above the test module in `crates/core/src/facts/exchange.rs`, so the file is the implementation followed by the tests from Step 2:

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p chess-analyzer-core --lib -- facts::exchange`
Expected: 7 passed. Compiler warnings that the two functions are never used are expected until Task 3.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/lib.rs crates/core/src/facts.rs crates/core/src/facts/exchange.rs
git commit -m "Add static exchange evaluation for commentary facts" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Forks and pins

**Files:**
- Create: `crates/core/src/facts/motifs.rs`
- Modify: `crates/core/src/facts.rs`

**Interfaces:**
- Consumes: `exchange::exchange_gain` (Task 1).
- Produces (`pub(super)`, used by Task 3):
  - `struct Fork { pub attacker: Role, pub targets: Vec<(Role, Square)> }` and `fork(before: &Chess, mv: Move) -> Option<Fork>`: did `mv` attack two or more enemy pieces at once with a piece that cannot be taken at a profit (the king counts as a target).
  - `struct Pin { pub slider: Role, pub pinned: (Role, Square) }` and `pin(before: &Chess, mv: Move) -> Option<Pin>`: did `mv` leave a bishop, rook or queen pinning a piece worth at least a minor piece against the enemy king (a move that gives check is not a pin).

- [ ] **Step 1: Declare the module**

In `crates/core/src/facts.rs`, add `mod motifs;` below `mod exchange;`.

- [ ] **Step 2: Write the failing tests**

Create `crates/core/src/facts/motifs.rs` containing only this test module:

```rust
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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p chess-analyzer-core --lib -- facts::motifs`
Expected: FAIL to compile (``cannot find function `fork` ``).

- [ ] **Step 4: Write the implementation**

Put this above the test module in `crates/core/src/facts/motifs.rs`:

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p chess-analyzer-core --lib -- facts::`
Expected: 12 passed (7 from Task 1, 5 new).

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/facts.rs crates/core/src/facts/motifs.rs
git commit -m "Add fork and pin detectors for commentary facts" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The digest

**Files:**
- Modify: `crates/core/src/review.rs` (the new field; `balance` visibility)
- Modify: `crates/cli/src/report.rs` (one test literal)
- Modify: `app/src/test-utils/liveEvents.ts` (keeps the TypeScript type check green)
- Regenerated by the test run: `app/src/generated/MoveReview.ts`, `app/src/fixtures/*.stored.json`
- Replace: `crates/core/src/facts.rs`
- Create: `crates/core/src/facts/king.rs`

**Interfaces:**
- Consumes: `exchange::loose_pieces` (Task 1), `motifs::fork` / `motifs::pin` (Task 2), `review::balance` (made `pub(crate)` here).
- Produces (used by Tasks 4 to 6):
  - `pub struct CommentaryInput<'a> { pub fen_before: &'a str, pub review: &'a MoveReview, pub reply_pv: &'a [String] }` (`Copy`).
  - `pub fn digest(input: &CommentaryInput<'_>) -> Option<Digest>`.
  - `pub struct Digest { ply, mover: Side, san, class: MoveClass, loss, eval_before, eval_after, played_is_best, best_san: Option<String>, played: MoveFacts, best: Option<MoveFacts>, played_line: Vec<String>, best_line: Vec<String>, material_now: i32, material_played: i32, material_best: Option<i32>, facts: Vec<Fact> }`, `Serialize`.
  - `pub enum Fact { MateAllowed, MateMissed, MaterialLost, Loose, ForcedKingMove, AllowsFork, AllowsPin, ForcesMate, WinsMaterial, Forks, Pins }` with `is_consequence()` and `weight()`.
  - `pub enum Kind` (with `name()`), `pub struct Spot { kind, square }`, `pub struct MoveFacts { kind, captures, check, mate, castles, promotes }`.
  - `MoveReview.commentary: Option<String>` (serde default).

- [ ] **Step 1: Add the field and keep everything green**

In `crates/core/src/review.rs`, add this field at the end of `MoveReview`:

```rust
    /// A few plain sentences about the move, written from the engine's own lines (see
    /// `commentary`). `None` for a review saved before commentary existed, or when the move's
    /// position could not be read.
    #[serde(default)]
    pub commentary: Option<String>,
```

Make `balance` visible to the new module: change `fn balance(` to `pub(crate) fn balance(`. In `review_move`, the struct literal ends with `critical: class.is_critical(),`; add `commentary: None,` after it. In `crates/cli/src/report.rs`, add `commentary: None,` after `critical: class.is_critical(),` in the test helper `mv`.

In `app/src/test-utils/liveEvents.ts`, give `review()` a final parameter and field:

```ts
  accuracy = 90,
  commentary: string | null = null,
): MoveReview {
```

and add `commentary,` after `critical: false,` in the returned object.

Regenerate what the field changes (the TypeScript type and the app fixtures gain `"commentary": null`):

Run: `UPDATE_GOLDEN=1 cargo test -p chess-analyzer-core --test golden`
Run: `cargo test`
Expected: all pass. `git status` shows `app/src/generated/MoveReview.ts`, `app/src/fixtures/fools_mate.stored.json` and `app/src/fixtures/opera_game.stored.json` changed. Check the type check too: `cd app && npx tsc --noEmit && cd ..` prints nothing.

- [ ] **Step 2: Write the failing tests**

Replace `crates/core/src/facts.rs` with the following: the two module declarations and the test module that the digest must satisfy. (The test module is long because it covers the Review Focus cases: a reply that stops being legal, no reply, special moves, mate in play.)

```rust
//! Facts about one move, worked out from the engine's own lines. (Implementation follows.)

mod exchange;
mod motifs;

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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p chess-analyzer-core --lib -- facts::tests`
Expected: FAIL to compile (``cannot find type `Digest` in this scope``, and the same for `MoveReview`, `Fact`, `Kind` and the others).

- [ ] **Step 4: Write the implementation**

Create `crates/core/src/facts/king.rs`:

```rust
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
```

Replace the top of `crates/core/src/facts.rs` (everything above `#[cfg(test)]`) with the following; keep the test module from Step 2 below it:

```rust
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p chess-analyzer-core --lib -- facts::`
Expected: 28 passed (12 from Tasks 1 and 2, 16 new).

Run: `cargo test -p chess-analyzer-core`
Expected: all pass, including the live-session tests (they still compare whole `MoveReview` values, which now carry `commentary: None`).

- [ ] **Step 6: Commit**

```bash
git add crates app data
git commit -m "Add the facts digest: material, mates, loose pieces, king safety, forks, pins" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The template renderer

**Files:**
- Create: `crates/core/src/commentary.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Digest`, `Fact`, `Spot`, `MoveFacts`, `CommentaryInput`, `digest` (Task 3).
- Produces (used by Task 5):
  - `pub fn render(d: &Digest) -> String`: two or three plain sentences.
  - `pub fn write(input: &CommentaryInput<'_>) -> Option<String>`: `digest(input).map(|d| render(&d))`.

- [ ] **Step 1: Declare the module**

In `crates/core/src/lib.rs`, add `pub mod commentary;` after `pub mod classify;`:

```rust
pub mod classify;
pub mod commentary;
pub mod engine;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/core/src/commentary.rs` containing only this test module:

```rust
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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p chess-analyzer-core --lib -- commentary::`
Expected: FAIL to compile (``cannot find function `render` ``).

- [ ] **Step 4: Write the implementation**

Put this above the test module in `crates/core/src/commentary.rs`:

```rust
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
            return format!(
                "{first} The evaluation goes from {} to {}.",
                eval_phrase(d.eval_before),
                eval_phrase(d.eval_after)
            );
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
        .facts
        .iter()
        .find(|f| !f.is_consequence())
        .map(|f| sentence(f, d))
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p chess-analyzer-core --lib -- commentary::`
Expected: 12 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/lib.rs crates/core/src/commentary.rs
git commit -m "Add the template commentary renderer" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Commentary in the review and live pipelines

**Files:**
- Modify: `crates/core/src/review.rs`
- Modify: `crates/core/src/store.rs`
- Modify: `crates/core/tests/golden.rs`
- Regenerated: `data/fixtures/fools_mate.golden.txt`, `data/fixtures/opera_game.golden.txt`, `app/src/fixtures/*.stored.json`

**Interfaces:**
- Consumes: `commentary::write`, `commentary::render`, `facts::digest`, `CommentaryInput`, `Digest` (Tasks 3 and 4).
- Produces:
  - `review_move` now sets `MoveReview.commentary`; the live session gets it for free because it calls `review_move`.
  - `pub fn digest_for(game: &Game, review: &Review, i: usize) -> Option<Digest>` (used by Task 6).
  - `pub fn backfill_commentary(game: &Game, review: &mut Review)` (used by `GameStore::get`).

- [ ] **Step 1: Write the failing tests**

In `crates/core/src/review.rs`, in the test `reviews_the_fools_mate`, after `assert_eq!(review.critical_plies, vec![1, 3]);` add:

```rust
        assert_eq!(
            review.moves[2].commentary.as_deref(),
            Some("g4 was a blunder; d4 was better. It allows Black to force checkmate in 1.")
        );
```

In `crates/core/src/store.rs`, add this test inside `mod tests` (before `side_is_part_of_the_stored_review_round_trip`):

```rust
    #[test]
    fn a_review_saved_before_commentary_existed_gets_it_when_loaded() {
        let store = GameStore::in_memory().unwrap();
        let game = parse_pgn("[White \"A\"]\n[Black \"B\"]\n\n1. e4 *")
            .unwrap()
            .remove(0);
        let (_, mut review) = sample("A");
        review.moves = vec![crate::review::MoveReview {
            ply: 1,
            move_number: 1,
            side: Side::White,
            san: "e4".into(),
            uci: "e2e4".into(),
            class: crate::classify::MoveClass::Best,
            eval_before: Eval::Cp(20),
            eval_after: Eval::Cp(20),
            best_uci: Some("e2e4".into()),
            best_san: Some("e4".into()),
            best_pv: vec!["e2e4".into()],
            loss: 0.0,
            accuracy: 100.0,
            critical: false,
            commentary: None,
        }];
        let id = store.save(&game, &review).unwrap();
        // Write the JSON the way an older version did: without a commentary key at all.
        let mut value = serde_json::to_value(&review).unwrap();
        value["moves"][0]
            .as_object_mut()
            .unwrap()
            .remove("commentary");
        store
            .conn
            .execute(
                "UPDATE games SET review_json = ?1 WHERE id = ?2",
                params![value.to_string(), id],
            )
            .unwrap();
        let loaded = store.get(id).unwrap().unwrap();
        let text = loaded.review.moves[0].commentary.as_deref();
        assert!(text.is_some_and(|t| t.starts_with("e4 is ")), "{text:?}");
    }
```

In `crates/core/tests/golden.rs`: change the `review` import to `use chess_analyzer_core::review::{Review, ReviewOptions, digest_for, review_game};`. In `snapshot`, after the `out.push_str(&format!("{:>3} {:<8} ...` call for each move, add the commentary line for critical moves:

```rust
        if m.critical {
            out.push_str(&format!(
                "      > {}\n",
                m.commentary.as_deref().unwrap_or("(no commentary)")
            ));
        }
```

and add the grounding test above `/// Wraps an analyzer and remembers every answer, in call order.`:

```rust
/// Every square written in `text`: a file letter a-h followed by a rank digit 1-8.
fn squares_in(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(2)
        .filter(|pair| ('a'..='h').contains(&pair[0]) && ('1'..='8').contains(&pair[1]))
        .map(|pair| pair.iter().collect())
        .collect()
}

/// The commentary may only say what the digest holds: every square it names, and every kind of
/// piece it names (the king aside, which sentences use for "the king is driven"), appears in the
/// digest of the same move.
#[test]
fn commentary_only_says_what_the_digest_holds() {
    let mut checked = 0;
    for name in FIXTURES {
        let (game, review) = replay(name);
        for (i, mv) in review.moves.iter().enumerate() {
            let Some(text) = &mv.commentary else {
                continue;
            };
            let d = digest_for(&game, &review, i).expect("a digest for a move that has commentary");
            let json = serde_json::to_string(&d).expect("serialize");
            for square in squares_in(text) {
                assert!(
                    json.contains(&square),
                    "{name} ply {}: the text names {square}, which the digest does not hold:\n{text}\n{json}",
                    mv.ply
                );
            }
            for piece in ["pawn", "knight", "bishop", "rook", "queen"] {
                if text.contains(piece) {
                    assert!(
                        json.contains(&format!("\"{piece}\"")),
                        "{name} ply {}: the text names a {piece}, which the digest does not hold:\n{text}",
                        mv.ply
                    );
                }
            }
            checked += 1;
        }
    }
    assert!(checked > 20, "only {checked} moves had commentary");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p chess-analyzer-core`
Expected: FAIL. `golden.rs` does not compile (``unresolved import `chess_analyzer_core::review::digest_for` ``). Run `cargo test -p chess-analyzer-core --lib` as well: `reviews_the_fools_mate` and `a_review_saved_before_commentary_existed_gets_it_when_loaded` fail because no commentary is written yet.

- [ ] **Step 3: Write the implementation**

In `crates/core/src/review.rs`, add these imports (keeping the others as they are):

```rust
use crate::commentary;
use crate::facts::{CommentaryInput, Digest, digest};
```

Replace the end of `review_move`, from `MoveReview {` (the struct literal that is the function's return value) through the closing brace, and add the two new functions after it. The whole region, from the `let mut review = MoveReview {` line to the end of `backfill_commentary`, reads:

```rust
    let mut review = MoveReview {
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
    };
    // The engine's expected reply to the played move is the best line of the position after it.
    let commentary = commentary::write(&CommentaryInput {
        fen_before: &game.positions[i],
        review: &review,
        reply_pv: &after_analysis.lines[0].pv,
    });
    review.commentary = commentary;
    review
}

/// The facts about move `i` (0-based) of a finished review, rebuilt from the game and the review
/// alone: the engine's reply to the move is the best line stored with the next move. `None` if the
/// move or its position cannot be read. This is what the CLI's `--facts` prints.
pub fn digest_for(game: &Game, review: &Review, i: usize) -> Option<Digest> {
    let mv = review.moves.get(i)?;
    let reply: Vec<String> = review
        .moves
        .get(i + 1)
        .map(|next| next.best_pv.clone())
        .unwrap_or_default();
    digest(&CommentaryInput {
        fen_before: game.positions.get(i)?,
        review: mv,
        reply_pv: &reply,
    })
}

/// Writes the commentary a review saved before commentary existed lacks, from the game and the
/// review alone (see `digest_for`; the last move has no next move, so it gets commentary without
/// reply facts). Moves that already have commentary are left alone.
pub fn backfill_commentary(game: &Game, review: &mut Review) {
    for i in 0..review.moves.len().min(game.moves.len()) {
        if review.moves[i].commentary.is_none() {
            let text = digest_for(game, review, i).map(|d| commentary::render(&d));
            review.moves[i].commentary = text;
        }
    }
}
```

In `crates/core/src/store.rs`, import `backfill_commentary` and use it in `get`:

```rust
use crate::review::{Accuracy, Review, backfill_commentary};

// ... in `GameStore::get`, replace the final `Ok(Some(StoredGame { ... }))` with:
        let game: Game = serde_json::from_str(&game_json)?;
        let mut review: Review = serde_json::from_str(&review_json)?;
        // Reviews saved before commentary existed get theirs now.
        backfill_commentary(&game, &mut review);
        Ok(Some(StoredGame {
            summary,
            game,
            review,
        }))
```

- [ ] **Step 4: Run the unit tests**

Run: `cargo test -p chess-analyzer-core --lib`
Expected: all pass (216 in total at this point, including the live-session tests, which compare live and finished reviews of the same analyses).

- [ ] **Step 5: Regenerate the golden files and read them**

Run: `UPDATE_GOLDEN=1 cargo test -p chess-analyzer-core --test golden`
Run: `cargo test -p chess-analyzer-core`
Expected: all pass. `git status` shows the two `.golden.txt` files and the two `.stored.json` files changed.

Read the new commentary lines in `data/fixtures/opera_game.golden.txt`. They are the product of this milestone, so check them with your own eyes. They should be exactly:

```text
      > Bxf3 is a mistake; Nc6 was better. The evaluation goes from slightly better for White to clearly better for White.
      > Nf6 is a mistake; Qf6 was better. After Qb3, the pawns on b7 and f7 are both attacked and short of protection. In the engine's line Qb3 Bc5 Bxf7+ Ke7, Black's king is driven to e7 and can no longer castle.
      > Qb3 is a great find. It wins 1 point of material in the engine's line.
      > b5 is a mistake; Kd8 was better. The evaluation goes from clearly better for White to winning for White.
      > Nxb5 is a great find. It captures the pawn.
      > cxb5 is an inaccuracy; Qb4+ was better. In the engine's line Bxb5+ Kd8, Black's king is driven to d8 and can no longer castle.
      > Bxb5+ is a great find. It wins 1 point of material in the engine's line.
      > Rd1 is a great find.
      > Nxd7 is a great move. It captures the bishop.
```

and in `data/fixtures/fools_mate.golden.txt`:

```text
      > g4 was a blunder; Nc3 was better. It allows Black to force checkmate in 1.
```

The `Nf6` line is the one the whole design exists for: it names the pawns left short of protection after `Qb3` and the king driven to e7, which material alone could not explain. Two lines (`Bxf3` and `b5`) fall back to "the evaluation goes from ... to ..." because no detector has a fact for them yet; that is the honest fallback and not a bug.

- [ ] **Step 6: Commit**

```bash
git add crates data app
git commit -m "Write commentary for every move in reviews and live games" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 6: CLI: `--commentary` and `--facts`

**Files:**
- Modify: `crates/cli/src/report.rs`
- Modify: `crates/cli/src/main.rs`

**Interfaces:**
- Consumes: `review::digest_for` (Task 5), `MoveReview.commentary`.
- Produces: `report::render_commentary(game: &Game, review: &Review, show_facts: bool) -> String`; two flags on `review`.

- [ ] **Step 1: Write the failing tests**

In `crates/cli/src/report.rs`, add inside `mod tests`, before `no_critical_moments_says_none`:

```rust
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
```

In `crates/cli/src/main.rs`, add inside `mod tests`, before `a_latin_1_pgn_file_is_read`:

```rust
    #[test]
    fn commentary_and_facts_are_optional_flags() {
        let Command::Review(plain) = parse(&[]).unwrap().command;
        assert!(!plain.commentary && !plain.facts);
        let Command::Review(asked) = parse(&["--commentary", "--facts"]).unwrap().command;
        assert!(asked.commentary && asked.facts);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p chess-analyzer-cli`
Expected: FAIL to compile (`render_commentary` and the `commentary` / `facts` fields do not exist).

- [ ] **Step 3: Write the implementation**

In `crates/cli/src/report.rs`, change the imports to:

```rust
use chess_analyzer_core::classify::MoveClass;
use chess_analyzer_core::eval::Side;
use chess_analyzer_core::game::Game;
use chess_analyzer_core::review::{MoveReview, Review, digest_for};
```

and add this function after `render`:

```rust
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
```

In `crates/cli/src/main.rs`, add the two flags to `ReviewArgs` after `json`:

```rust
    /// Also print the commentary for each critical move.
    #[arg(long)]
    commentary: bool,
    /// Also print the ranked facts each commentary was written from (implies --commentary).
    #[arg(long)]
    facts: bool,
```

and print the section after the report in `review()`:

```rust
        print!("{}", report::render(&review));
        if args.commentary || args.facts {
            print!("{}", report::render_commentary(game, &review, args.facts));
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p chess-analyzer-cli`
Expected: 9 passed.

- [ ] **Step 5: Try it on a real game**

Run: `cargo run -p chess-analyzer-cli -- review data/fixtures/opera_game.pgn --depth 12 --facts`
Expected: the usual report, then a `Commentary` section whose `6... Nf6` entry reads like the golden line above and is followed by a `facts: [{"fact":"loose",...` line. (Depth 12 may word a few other moves differently from the recorded depth-14 analyses; that is the engine, not the code.)

- [ ] **Step 6: Commit**

```bash
git add crates/cli
git commit -m "Add --commentary and --facts to the review command" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Show it in the app

**Files:**
- Modify: `app/src/lib/commentary.ts`, `app/src/lib/commentary.test.ts`
- Modify: `app/src/screens/ReviewScreen.tsx`, `ReviewScreen.test.tsx`
- Modify: `app/src/screens/LiveScreen.tsx`, `LiveScreen.test.tsx`
- Modify: `app/src/styles.css`

**Interfaces:**
- Consumes: `MoveReview.commentary: string | null` (generated type, Task 3), the `review()` test helper's new `commentary` parameter (Task 3).
- Produces: `commentaryFor(move: MoveReview | null, ply: number): string`.

- [ ] **Step 1: Write the failing tests**

In `app/src/lib/commentary.test.ts`, change the import to `import { commentaryFor, describeMove } from "./commentary";` and add at the end:

```ts
describe("commentaryFor", () => {
  it("prefers the commentary written from the engine's lines", () => {
    const written = { ...base, commentary: "g4 was a blunder; Nc3 was better. It allows Black to force checkmate in 1." };
    expect(commentaryFor(written, 3)).toBe(written.commentary);
  });

  it("falls back to the short sentence when a move has no commentary", () => {
    expect(commentaryFor({ ...base, commentary: null }, 3)).toBe(describeMove(base, 3));
    expect(commentaryFor(null, 3)).toBe("Analysing this move…");
    expect(commentaryFor(null, 0)).toBe("The starting position.");
  });
});
```

In `app/src/screens/ReviewScreen.test.tsx`, the first screen test looks for the old sentence. The regenerated fixture now carries commentary, so change the expectation:

```tsx
    expect(screen.getByText(/^g4 was a blunder; Nc3 was better\./)).toBeInTheDocument();
```

In `app/src/screens/LiveScreen.test.tsx`, add this block just before `describe("LiveScreen", () => {`:

```tsx
describe("LiveScreen commentary", () => {
  const WRITTEN = "e5 is a mistake; c5 was better. After Nf3, the pawn on e5 is attacked and short of protection.";
  const withCommentary = (provisional: boolean) =>
    liveState(MOVES, [
      position(1, 0, "e2e4", cp(30)),
      position(1, 1, "e7e5", cp(25)),
      position(1, 2, "g1f3", cp(31)),
      move(1, review(1, "e2e4", "best", "e2e4"), false),
      move(1, review(2, "e7e5", "mistake", "c7c5", 90, WRITTEN), provisional),
    ]);

  it("shows the commentary for the latest move, and says a provisional one may change", () => {
    render(<Harness live={withCommentary(true)} initial={withMoves(...MOVES)} />);
    expect(screen.getByText(/^e5 is a mistake; c5 was better\./)).toBeInTheDocument();
    expect(screen.getByText(/This may change as the engine searches deeper\./)).toBeInTheDocument();
  });

  it("does not hedge a move whose class is settled", () => {
    render(<Harness live={withCommentary(false)} initial={withMoves(...MOVES)} />);
    expect(screen.getByText(/^e5 is a mistake; c5 was better\./)).toBeInTheDocument();
    expect(screen.queryByText(/This may change/)).not.toBeInTheDocument();
  });

  it("falls back to the short sentence while a move has no commentary", () => {
    render(<Harness live={ANALYSED} initial={withMoves(...MOVES)} />);
    expect(screen.getByText(/^e7e5 is a mistake\./)).toBeInTheDocument();
  });

  it("says a move is being analysed before the engine has classified it, and shows nothing at the start", () => {
    const { unmount } = render(<Harness initial={withMoves("e2e4")} />);
    expect(screen.getByText("Analysing this move…")).toBeInTheDocument();
    unmount();
    render(<Harness />);
    expect(screen.queryByText("Analysing this move…")).not.toBeInTheDocument();
    expect(screen.queryByText("The starting position.")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app && npx vitest run src/lib/commentary.test.ts src/screens/ReviewScreen.test.tsx src/screens/LiveScreen.test.tsx`
Expected: FAIL (`commentaryFor` is not exported; the Live screen shows no commentary line).

- [ ] **Step 3: Write the implementation**

In `app/src/lib/commentary.ts`, replace the doc comment above `describeMove` with:

```ts
/**
 * One plain sentence about a move, from the classification alone. It is what the screens show
 * until the move has commentary of its own (see `commentaryFor`).
 */
```

and add `commentaryFor` at the end of the file:

```ts
/**
 * What to show under the board for a move: the commentary the backend wrote from the engine's
 * lines when there is some, otherwise the short sentence built from the classification (a review
 * saved before commentary existed, a move still being analysed, or a position that could not be
 * read).
 */
export function commentaryFor(move: MoveReview | null, ply: number): string {
  return move?.commentary ?? describeMove(move, ply);
}
```

In `app/src/screens/ReviewScreen.tsx`, import and use it:

```tsx
import { commentaryFor } from "../lib/commentary";
```

```tsx
            {commentaryFor(ply === 0 ? null : data.moves[ply - 1], ply)}
```

In `app/src/screens/LiveScreen.tsx`, add the import (below the `useBoardEntry` import) and the line under the status text:

```tsx
import { commentaryFor } from "../lib/commentary";
```

```tsx
          {ply > 0 && (
            <p className="review__commentary live__commentary" aria-live="polite">
              {commentaryFor(view.data.moves[ply - 1], ply)}
              {view.data.moves[ply - 1] && view.provisional[ply - 1] && (
                <span className="live__provisional"> This may change as the engine searches deeper.</span>
              )}
            </p>
          )}
```

In `app/src/styles.css`, add before `.live__hint {`:

```css
.live__provisional {
  color: var(--muted);
}
```

- [ ] **Step 4: Run the tests and the type check**

Run: `cd app && npx vitest run && npx tsc --noEmit`
Expected: all test files pass (302 tests) and `tsc` prints nothing.

- [ ] **Step 5: Commit**

```bash
git add app
git commit -m "Show engine-based commentary in the review and live screens" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Documentation and final verification

**Files:**
- Modify: `README.md`, `AGENTS.md`

- [ ] **Step 1: README**

In `README.md`, replace the sentence `LLM commentary comes next.` in the Status paragraph with:

```
Every move also gets a plain-language explanation written from the engine's own lines (no model needed); an optional local-LLM coach comes later.
```

Add these two rows to the options table, after the `--json` row:

```
| `--commentary` | off | Also print the plain-language explanation under each critical moment. |
| `--facts` | off | Also print the ranked facts each explanation was written from, as JSON (implies `--commentary`). |
```

- [ ] **Step 2: AGENTS.md**

Insert this subsection immediately before the `## LLM Integration` heading:

```
### Commentary

Every move also gets a few plain sentences, written from the engine's own lines with no model involved. `crates/core/src/facts.rs` works out a ranked list of verifiable facts (material along the lines, forced mates, pieces left short of protection, a king driven out of castling, forks and pins) and `crates/core/src/commentary.rs` turns them into text. `review::review_move` stores the result in `MoveReview.commentary`, so finished reviews and live games share it. The local LLM described below is an optional second renderer over the same facts; see `docs/superpowers/specs/2026-10-09-move-commentary-design.md`.
```

- [ ] **Step 3: Run everything CI runs**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cd app && npm test && npm run build && cd ..
git diff --exit-code -- app/src/generated app/src/fixtures
```

Expected: every command succeeds and the last prints nothing. If you have the Tauri toolchain, also run `cargo build -p chess-analyzer-app` (CI does); this plan does not touch that crate.

- [ ] **Step 4: Look at it in the app**

Run: `cd app && npm run tauri dev`. Open the Games screen, review `data/fixtures/opera_game.pgn` (or open a saved review), and step to move 6 for Black (`Nf6`). The line under the board should read: *Nf6 is a mistake; Qf6 was better. After Qb3, the pawns on b7 and f7 are both attacked and short of protection. In the engine's line Qb3 Bc5 Bxf7+ Ke7, Black's king is driven to e7 and can no longer castle.* Then open Live, enter `e4 e5 Nf3`, and check that a sentence appears under the status line once Stockfish has classified a move, with "This may change as the engine searches deeper." while the newest move is still provisional.

- [ ] **Step 5: Commit**

```bash
git add README.md AGENTS.md
git commit -m "Document the commentary" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage (Milestone 1):** facts digest with typed ranked facts (Task 3); material along the lines, exchange-aware loose pieces including batteries, king safety, mates, fork and pin (Tasks 1 to 3); template renderer with verdict, causes, better move, honest fallback and per-ply phrasing (Task 4); review and live share it through `review_move`, old reviews are back-filled (Task 5); CLI `--commentary` and `--facts` (Task 6); both screens, fallback, provisional note (Task 7); golden commentary and the grounding test (Task 5); generated-type drift check (Task 3, Task 8). The spec's Tauri command was replaced by the `MoveReview.commentary` field, and the spec was updated to say so. Back-rank weakness and discovered attack are deliberately left for later, as the spec's detector list allows ("follow the same pattern later").

**Placeholders:** none; every code step carries the code.

**Type consistency:** `CommentaryInput`, `Digest`, `Fact` variants, `Kind`, `Spot` and `MoveFacts` are defined in Task 3 and used with the same names in Tasks 4 to 6; `digest_for` and `backfill_commentary` are defined in Task 5 and used in Tasks 5 and 6; `commentaryFor` is defined and used in Task 7.

---

## Changes made after the whole-branch review

A fresh reviewer read the finished branch and found four accuracy problems that the plan's tests did not cover. Each was fixed test-first (the test failed, then passed) in one pass; the code in the tasks above is the code as first written, and the repository holds the corrected version.

- **A mating move is never called a fork or a pin**, and a mating move always says "It delivers checkmate." first (`facts.rs`: the played move's fork and pin facts are skipped when it gives mate; `commentary.rs`: mate first).
- **The best line is the baseline for every consequence.** A piece, a forced king move, a fork or a pin is only blamed on the played move when the engine's best line does not have it too. For `6...Nf6` this removed `b7` (the best move `6...Qf6` loses it as well), so the sentence now reads: *After Qb3, the pawn on f7 is attacked and short of protection. In the engine's line Qb3 Bc5 Bxf7+ Ke7, Black's king is driven to e7 and can no longer castle.*
- **Material facts need a horizon of at least two plies**, so a capture that is simply recaptured is no longer called a win.
- **The fallback sentence gives the numbers** when both evaluations fall in the same wording band ("from +2.00 to +2.50 (from White's point of view)").

- **The live "This may change as the engine searches deeper." note was removed** (the provisional badge already says so); the Live screen test now asserts that no such note appears.

Not changed: exchange evaluation still ignores absolute pins (a documented approximation).
