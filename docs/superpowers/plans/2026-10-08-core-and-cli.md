# chess-analyzer Core + CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `core` crate and a command-line reviewer: give it a PGN and a local Stockfish, get back every move classified, per-player accuracy, an eval series, critical moments and the opening name.

**Architecture:** A Cargo workspace with a UI-free `core` library and a thin `cli` binary. `core` talks to Stockfish over UCI behind an `Analyzer` trait, so the review pipeline, the SQLite cache and the tests all work against that one seam (a scripted fake in unit tests, the real engine in integration tests). Classification is pure functions over numbers (win percentages), with every threshold in one `Thresholds` struct.

**Tech Stack:** Rust 2024 (floor 1.88, built and verified on 1.99), `shakmaty` 0.30.2, `pgn-reader` 0.29.1, `rusqlite` 0.40.2 (bundled SQLite), `serde`/`serde_json`, `thiserror` 2, `clap` 4 + `anyhow` (CLI only). Stockfish `sf_19` ("universal" build) as a separately downloaded process.

**Spec:** `docs/superpowers/specs/2026-10-08-chess-analyzer-v1-design.md` (this plan is build-order step 1, "`core` + CLI"). Architecture background is in `AGENTS.md`.

**Not in this plan:** the Tauri/React app, record-by-hand mode in the UI, LLM commentary, variations. `Game::from_uci_moves` exists here only because record mode will need it.

## Global Constraints

- Open source, MIT licensed (`LICENSE` already in the repo); personal-first, so no installers, signing or verified macOS/Linux support.
- Repo layout: `crates/core`, `crates/cli`, `data/`, `scripts/`, with `app/` reserved for the later Tauri app.
- Stockfish is the authority on chess; `core` contains no LLM code. Stockfish is run as a native UCI process, never as WASM.
- Evaluation is `Cp(i32)` or `Mate(i32)` (plus `Checkmate(Side)` for a finished game), always stored from White's point of view.
- All classification thresholds live in one configurable struct (`Thresholds`); the defaults are a starting point to be calibrated on real games.
- Engine defaults: depth 20, MultiPV 3, one thread, 256 MB hash; all configurable.
- Stockfish is fetched by a setup script into `engines/` (gitignored), never committed; an explicit path (`--engine` / `STOCKFISH_PATH`) overrides it.
- A game is a single linear sequence of moves (no variations). A PGN with several games: the user picks one.
- No network access at runtime. The opening data is bundled into the binary.
- Development and testing happen on Windows; keep the code portable.
- Errors must leave the tool usable: a dead or hung engine is restarted and the position retried once; an unusable cache means "carry on uncached" with a warning.

## Review Focus

Inputs the spec implies but no obvious task would exercise; each line names the test that pins it.

1. **A PGN that starts from a custom position with Black to move** (`[FEN "..."]`): sides and move numbers must follow the FEN, not assume White starts. Test: `a_game_from_a_black_to_move_position_is_numbered_from_its_fen` (Task 8).
2. **Input that is not really a PGN** (pasted HTML or prose, a file with a UTF-8 BOM, Windows line endings, non-ASCII player names): garbage must be an error, never an empty "review"; the rest must parse. Tests: `garbage_text_is_an_error_not_an_empty_game`, `windows_line_endings_are_accepted`, `a_utf8_byte_order_mark_at_the_start_is_tolerated`, `non_ascii_player_names_survive`, `games_without_moves_are_skipped` (Task 3).
3. **The engine hangs or is too slow**: the call must time out, the process must be replaced, and the engine must still answer afterwards. Test: `a_timeout_restarts_the_engine_and_surfaces_an_error` (Task 11).
4. **The same position reached with different move counters** (transposition, repetition, a second game sharing an opening): it must hit the cache. Tests: `positions_that_differ_only_in_move_counters_share_an_entry`, `a_different_side_to_move_is_a_different_position` (Task 9).
5. **A named opening line that contains a blunder** (the dataset includes "Fool's Mate"), and **games that end in stalemate or have no moves**: book status must not hide a blunder, and a finished game must never be sent to the engine. Tests: `a_book_move_that_loses_badly_is_not_book` (Task 6), `a_blunder_inside_a_known_line_is_still_flagged` (Task 8), `a_stalemate_final_position_is_scored_as_equal`, `a_game_with_no_moves_reviews_to_an_empty_review` (Task 8).

## Conventions for every task

- Run commands from the repository root. Shell snippets are for Git Bash (Windows) or any POSIX shell; the Stockfish setup on Windows uses PowerShell and says so.
- Test-first for modules: Step 1 writes the test module, Step 2 proves it fails to compile or run, Step 3 adds the implementation *above* the `#[cfg(test)]` line, Step 4 proves it passes.
- Code is `cargo fmt` formatted (edition 2024 style). Run `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` before every commit; both are clean in the final code.
- Deliberate deviations from `AGENTS.md`/the spec, all recorded in Task 13: win-percentage thresholds instead of the centipawn table; a `Checkmate` evaluation variant; a `book_max_loss` rule so a named line cannot hide a blunder.

## File Structure

```
Cargo.toml                       workspace manifest
.gitignore / .gitattributes      Rust + Stockfish ignores; LF for scripts and data
README.md
crates/core/
  Cargo.toml
  src/lib.rs                     module list
  src/eval.rs                    Side, Eval, win probability, per-move accuracy
  src/game.rs                    Game, PGN parsing, building from UCI moves
  src/engine.rs                  Analyzer trait, UCI parsing, UciEngine, ScriptedAnalyzer
  src/classify.rs                MoveClass, Thresholds, MoveContext, classify()
  src/openings.rs                OpeningBook (names + book positions)
  src/review.rs                  review_game(): Game + Analyzer -> Review
  src/cache.rs                   SQLite CachedAnalyzer decorator
  tests/stockfish.rs             integration tests against a real Stockfish
  tests/golden.rs                golden review snapshots + fixture recorder
crates/cli/
  Cargo.toml
  src/main.rs                    `chess-analyzer review ...`
  src/report.rs                  plain-text rendering of a Review
data/openings.tsv                lichess-org/chess-openings (CC0)
data/README.md
data/fixtures/                   fixture PGNs, recorded analyses, golden snapshots
scripts/setup-stockfish.ps1      Windows
scripts/setup-stockfish.sh       Linux / macOS
.github/workflows/ci.yml
```

---

### Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml`, `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`, `.gitattributes`
- Modify: `.gitignore` (append a Rust section; the existing file is the Node template)

**Interfaces:**
- Produces: a workspace that builds. Package names `chess-analyzer-core` (library, crate name `chess_analyzer_core`) and `chess-analyzer-cli` (binary `chess-analyzer`).

- [ ] **Step 1: Create the workspace manifest**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/core", "crates/cli"]

[workspace.package]
edition = "2024"
license = "MIT"
version = "0.1.0"
rust-version = "1.88"
```

- [ ] **Step 2: Create the two crates**

`crates/core/Cargo.toml`:

```toml
[package]
name = "chess-analyzer-core"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
pgn-reader = "0.29.1"
rusqlite = { version = "0.40.2", features = ["bundled"] }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
shakmaty = "0.30.2"
thiserror = "2.0.21"
```

`crates/core/src/lib.rs` (modules are added task by task; start with just the doc comment):

```rust
//! chess-analyzer core: everything about a chess game review that does not involve a UI.
```

`crates/cli/Cargo.toml`:

```toml
[package]
name = "chess-analyzer-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[[bin]]
name = "chess-analyzer"
path = "src/main.rs"

[dependencies]
anyhow = "1.0.104"
chess-analyzer-core = { path = "../core" }
clap = { version = "4.6.7", features = ["derive"] }
serde_json = "1.0.151"
```

`crates/cli/src/main.rs`:

```rust
fn main() {}
```

- [ ] **Step 3: Ignore build output, the local engine and the cache; pin line endings**

Append to `.gitignore`:

```

# Rust
/target

# Local Stockfish binary (scripts/setup-stockfish) and the CLI's analysis cache
/engines/
chess-analyzer-cache.db
```

Create `.gitattributes`:

```
# Keep scripts and data files LF on every platform (a CRLF shell script fails on Linux).
*.sh   text eol=lf
*.tsv  text eol=lf
*.json text eol=lf
*.pgn  text eol=lf
*.txt  text eol=lf
```

- [ ] **Step 4: Verify the workspace builds**

Run: `cargo check --workspace`
Expected: `Finished` with no errors (this downloads and compiles the dependencies once; allow a few minutes).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates .gitignore .gitattributes
git commit -m "Scaffold the Rust workspace"
```

---

### Task 2: Evaluations, win probability and accuracy (`eval`)

**Files:**
- Create: `crates/core/src/eval.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces:
  - `enum Side { White, Black }` with `fn opposite(self) -> Side` and `From<shakmaty::Color>`
  - `enum Eval { Cp(i32), Mate(i32), Checkmate(Side) }` (White's point of view; `Mate(n)` is positive when White mates, never zero), serde-tagged as `{"kind":"cp","value":12}`
  - `enum UciScore { Cp(i32), Mate(i32) }` and `Eval::from_uci(score: UciScore, side_to_move: Side) -> Eval` (UCI scores are relative to the side to move; `Mate(0)` means the side to move is already checkmated)
  - `Eval::win_percent(self) -> f64` (White, 0..=100), `win_percent_for(self, Side) -> f64`, `is_mate_for(self, Side) -> bool`, `is_mate_against(self, Side) -> bool`, `display(self) -> String` (`+0.34`, `M3`, `-M3`)
  - `fn move_accuracy(win_before: f64, win_after: f64) -> f64` (0..=100)

The win-probability curve and the accuracy formula are the ones Lichess publishes (constants `0.00368208`, `103.1668`, `-0.04354`, `-3.1668`). They are written from the published formulas and not independently re-derived here; the tests pin their behaviour (symmetry, monotonicity, one pawn is about 59%).

- [ ] **Step 1: Write the tests**

Create `crates/core/src/eval.rs` containing only:

```rust
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
```

- [ ] **Step 2: Register the module and watch it fail**

Add to `crates/core/src/lib.rs`:

```rust
pub mod eval;
```

Run: `cargo test -p chess-analyzer-core --lib eval::`
Expected: compile errors such as `cannot find type 'Eval' in this scope` (nothing is implemented yet).

- [ ] **Step 3: Implement**

Put this at the top of `crates/core/src/eval.rs`, above the `#[cfg(test)]` line:

```rust
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
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p chess-analyzer-core --lib eval::`
Expected: `11 passed; 0 failed`.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/core
git commit -m "Add evaluations, win probability and move accuracy"
```

---

### Task 3: Games and PGN parsing (`game`)

**Files:**
- Create: `crates/core/src/game.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `eval::Side`.
- Produces:
  - `struct GameMove { san: String, uci: String }`
  - `struct Game { headers: BTreeMap<String, String>, positions: Vec<String>, moves: Vec<GameMove> }` where `positions[i]` is the FEN before `moves[i]` and the last entry is the final position (`positions.len() == moves.len() + 1`)
  - `Game::side_to_move(&self, index: usize) -> Side`, `Game::position(&self, index: usize) -> shakmaty::Chess`
  - `Game::from_uci_moves(start_fen: Option<&str>, uci_moves: &[String], headers: BTreeMap<String, String>) -> Result<Game, GameError>`
  - `fn parse_pgn(text: &str) -> Result<Vec<Game>, GameError>`: variations and comments are ignored, games without moves are skipped, and the result always holds at least one game with a move
  - `enum GameError { Empty, Read(String), InvalidFen(String), IllegalMove { ply: usize, mv: String } }` (derives `PartialEq`)

The `pgn-reader` crate is lenient: arbitrary prose parses as a game with no moves, which is why `parse_pgn` drops move-less games and reports `Empty` (Review Focus 2).

- [ ] **Step 1: Write the tests**

Create `crates/core/src/game.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::Position;

    const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn parses_a_simple_game_with_headers() {
        let pgn =
            "[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n";
        let games = parse_pgn(pgn).unwrap();
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.headers["White"], "Alice");
        assert_eq!(g.moves.len(), 4);
        assert_eq!(g.positions.len(), 5);
        assert_eq!(g.positions[0], START_FEN);
        assert_eq!(
            g.moves[0],
            GameMove {
                san: "e4".into(),
                uci: "e2e4".into()
            }
        );
        assert_eq!(g.moves[2].san, "Nf3");
        assert_eq!(g.moves[2].uci, "g1f3");
    }

    #[test]
    fn side_to_move_alternates() {
        let g = &parse_pgn("1. e4 e5 *").unwrap()[0];
        assert_eq!(g.side_to_move(0), Side::White);
        assert_eq!(g.side_to_move(1), Side::Black);
    }

    #[test]
    fn castling_and_promotion_use_standard_uci() {
        let pgn = "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. O-O Nf6 *";
        let g = &parse_pgn(pgn).unwrap()[0];
        assert_eq!(g.moves[6].san, "O-O");
        assert_eq!(g.moves[6].uci, "e1g1");

        let promo = "[FEN \"7k/P7/8/8/8/8/8/K7 w - - 0 1\"]\n\n1. a8=Q+ *";
        let g = &parse_pgn(promo).unwrap()[0];
        assert_eq!(g.moves[0].uci, "a7a8q");
        assert_eq!(g.moves[0].san, "a8=Q+");
    }

    #[test]
    fn variations_and_comments_are_ignored() {
        let pgn = "1. e4 {best by test} e5 (1... c5 2. Nf3) 2. Nf3 *";
        let g = &parse_pgn(pgn).unwrap()[0];
        let sans: Vec<_> = g.moves.iter().map(|m| m.san.as_str()).collect();
        assert_eq!(sans, ["e4", "e5", "Nf3"]);
    }

    #[test]
    fn multiple_games_are_all_returned() {
        let pgn = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 d5 *\n";
        let games = parse_pgn(pgn).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[1].headers["Event"], "B");
        assert_eq!(games[1].moves.len(), 2);
    }

    #[test]
    fn illegal_move_reports_the_ply() {
        let err = parse_pgn("1. e4 e5 2. Ke3 *").unwrap_err();
        assert_eq!(
            err,
            GameError::IllegalMove {
                ply: 3,
                mv: "Ke3".into()
            }
        );
    }

    #[test]
    fn empty_input_is_an_error() {
        assert_eq!(parse_pgn("   \n").unwrap_err(), GameError::Empty);
    }

    #[test]
    fn invalid_fen_header_is_an_error() {
        let err = parse_pgn("[FEN \"not a fen\"]\n\n1. e4 *").unwrap_err();
        assert!(matches!(err, GameError::InvalidFen(_)));
    }

    #[test]
    fn checkmate_game_ends_in_a_final_position() {
        let g = &parse_pgn("1. f3 e5 2. g4 Qh4# 0-1").unwrap()[0];
        assert_eq!(g.moves.len(), 4);
        assert!(g.position(4).is_checkmate());
        assert_eq!(g.moves[3].san, "Qh4#");
    }

    #[test]
    fn garbage_text_is_an_error_not_an_empty_game() {
        assert_eq!(
            parse_pgn("hello, this is not chess at all"),
            Err(GameError::Empty)
        );
        assert_eq!(
            parse_pgn("<html><body>404</body></html>"),
            Err(GameError::Empty)
        );
    }

    #[test]
    fn windows_line_endings_are_accepted() {
        let pgn = "[White \"Alice\"]\r\n[Black \"Bob\"]\r\n\r\n1. e4 e5\r\n2. Nf3 Nc6 *\r\n";
        let g = &parse_pgn(pgn).unwrap()[0];
        assert_eq!(g.headers["Black"], "Bob");
        assert_eq!(g.moves.len(), 4);
    }

    #[test]
    fn non_ascii_player_names_survive() {
        let pgn = "[White \"Zoë Müller\"]\n[Black \"李雷\"]\n\n1. e4 *\n";
        let g = &parse_pgn(pgn).unwrap()[0];
        assert_eq!(g.headers["White"], "Zoë Müller");
        assert_eq!(g.headers["Black"], "李雷");
    }

    #[test]
    fn a_utf8_byte_order_mark_at_the_start_is_tolerated() {
        let pgn = "\u{feff}[White \"Alice\"]\n\n1. e4 e5 *\n";
        let g = &parse_pgn(pgn).unwrap()[0];
        assert_eq!(g.moves.len(), 2);
    }

    #[test]
    fn games_without_moves_are_skipped() {
        let pgn = "[Event \"A\"]\n\n*\n\n[Event \"B\"]\n\n1. e4 *\n";
        let games = parse_pgn(pgn).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].headers["Event"], "B");
    }
    #[test]
    fn builds_a_game_from_uci_moves() {
        let moves: Vec<String> = ["e2e4", "e7e5", "g1f3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let g = Game::from_uci_moves(None, &moves, BTreeMap::new()).unwrap();
        assert_eq!(
            g.moves.iter().map(|m| m.san.as_str()).collect::<Vec<_>>(),
            ["e4", "e5", "Nf3"]
        );
        assert_eq!(g.positions.len(), 4);
    }

    #[test]
    fn from_uci_moves_rejects_illegal_moves() {
        let moves = vec!["e2e5".to_string()];
        let err = Game::from_uci_moves(None, &moves, BTreeMap::new()).unwrap_err();
        assert_eq!(
            err,
            GameError::IllegalMove {
                ply: 1,
                mv: "e2e5".into()
            }
        );
    }
}
```

- [ ] **Step 2: Register the module and watch it fail**

Add `pub mod game;` to `crates/core/src/lib.rs` (keep the modules alphabetical).

Run: `cargo test -p chess-analyzer-core --lib game::`
Expected: compile errors such as `cannot find function 'parse_pgn' in this scope`.

- [ ] **Step 3: Implement**

Put this at the top of `crates/core/src/game.rs`, above the `#[cfg(test)]` line:

```rust
//! Games: PGN parsing and construction from a list of moves.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::ops::ControlFlow;

use pgn_reader::{RawTag, Reader, SanPlus, Visitor};
use serde::{Deserialize, Serialize};
use shakmaty::fen::Fen;
use shakmaty::san::SanPlus as ShakSanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, EnPassantMode};
use thiserror::Error;

use crate::eval::Side;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GameError {
    #[error("no game found in the PGN")]
    Empty,
    #[error("could not read the PGN: {0}")]
    Read(String),
    #[error("invalid FEN: {0}")]
    InvalidFen(String),
    #[error("illegal or unreadable move {mv:?} at ply {ply}")]
    IllegalMove { ply: usize, mv: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameMove {
    pub san: String,
    pub uci: String,
}

/// A single linear game. `positions[i]` is the FEN before `moves[i]`;
/// the last entry is the final position, so `positions.len() == moves.len() + 1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub headers: BTreeMap<String, String>,
    pub positions: Vec<String>,
    pub moves: Vec<GameMove>,
}

impl Game {
    /// The side that makes `moves[index]`.
    pub fn side_to_move(&self, index: usize) -> Side {
        if self.positions[index].split(' ').nth(1) == Some("b") {
            Side::Black
        } else {
            Side::White
        }
    }

    /// Parses the position at `index` (0 = start, `moves.len()` = final).
    pub fn position(&self, index: usize) -> Chess {
        parse_fen(&self.positions[index]).expect("positions are produced from valid FENs")
    }

    /// Builds a game from UCI moves, e.g. `["e2e4", "e7e5"]`. Used for hand-recorded games.
    pub fn from_uci_moves(
        start_fen: Option<&str>,
        uci_moves: &[String],
        headers: BTreeMap<String, String>,
    ) -> Result<Game, GameError> {
        let start = match start_fen {
            Some(fen) => parse_fen(fen)?,
            None => Chess::default(),
        };
        let mut builder = Builder::new(start, headers);
        for (i, text) in uci_moves.iter().enumerate() {
            let ply = i + 1;
            let uci = UciMove::from_ascii(text.as_bytes()).map_err(|_| GameError::IllegalMove {
                ply,
                mv: text.clone(),
            })?;
            let mv = uci
                .to_move(&builder.pos)
                .map_err(|_| GameError::IllegalMove {
                    ply,
                    mv: text.clone(),
                })?;
            builder.push(mv);
        }
        Ok(builder.finish())
    }
}

/// Parses the games in the PGN text. Variations and comments are ignored. Games without any
/// moves (including whatever the lenient reader makes of non-PGN text) are skipped, so a
/// successful result always holds at least one game with a move.
pub fn parse_pgn(text: &str) -> Result<Vec<Game>, GameError> {
    let mut reader = Reader::new(Cursor::new(text.as_bytes()));
    let mut games = Vec::new();
    let mut visitor = GameVisitor;
    while let Some(result) = reader
        .read_game(&mut visitor)
        .map_err(|e| GameError::Read(e.to_string()))?
    {
        games.push(result?);
    }
    games.retain(|game| !game.moves.is_empty());
    if games.is_empty() {
        return Err(GameError::Empty);
    }
    Ok(games)
}

fn parse_fen(fen: &str) -> Result<Chess, GameError> {
    fen.parse::<Fen>()
        .map_err(|e| GameError::InvalidFen(e.to_string()))?
        .into_position(CastlingMode::Standard)
        .map_err(|e| GameError::InvalidFen(e.to_string()))
}

fn fen_string(pos: &Chess) -> String {
    Fen::from_position(pos, EnPassantMode::Legal).to_string()
}

struct Builder {
    pos: Chess,
    headers: BTreeMap<String, String>,
    positions: Vec<String>,
    moves: Vec<GameMove>,
}

impl Builder {
    fn new(start: Chess, headers: BTreeMap<String, String>) -> Builder {
        let positions = vec![fen_string(&start)];
        Builder {
            pos: start,
            headers,
            positions,
            moves: Vec::new(),
        }
    }

    fn push(&mut self, mv: shakmaty::Move) {
        let uci = mv.to_uci(CastlingMode::Standard).to_string();
        let san = ShakSanPlus::from_move_and_play_unchecked(&mut self.pos, mv).to_string();
        self.positions.push(fen_string(&self.pos));
        self.moves.push(GameMove { san, uci });
    }

    fn finish(self) -> Game {
        Game {
            headers: self.headers,
            positions: self.positions,
            moves: self.moves,
        }
    }
}

struct GameVisitor;

struct Tags {
    headers: BTreeMap<String, String>,
    start: Option<Chess>,
    error: Option<GameError>,
}

struct Movetext {
    builder: Option<Builder>,
    error: Option<GameError>,
}

impl Visitor for GameVisitor {
    type Tags = Tags;
    type Movetext = Movetext;
    type Output = Result<Game, GameError>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(Tags {
            headers: BTreeMap::new(),
            start: None,
            error: None,
        })
    }

    fn tag(
        &mut self,
        tags: &mut Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        let name = String::from_utf8_lossy(name).into_owned();
        let value = value.decode_utf8_lossy().into_owned();
        if name == "FEN" {
            match parse_fen(&value) {
                Ok(pos) => tags.start = Some(pos),
                Err(e) => tags.error = Some(e),
            }
        }
        tags.headers.insert(name, value);
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, tags: Tags) -> ControlFlow<Self::Output, Movetext> {
        if let Some(error) = tags.error {
            return ControlFlow::Continue(Movetext {
                builder: None,
                error: Some(error),
            });
        }
        let start = tags.start.unwrap_or_default();
        ControlFlow::Continue(Movetext {
            builder: Some(Builder::new(start, tags.headers)),
            error: None,
        })
    }

    fn san(&mut self, movetext: &mut Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        let Some(builder) = movetext.builder.as_mut() else {
            return ControlFlow::Continue(());
        };
        let ply = builder.moves.len() + 1;
        match san_plus.san.to_move(&builder.pos) {
            Ok(mv) => {
                builder.push(mv);
                ControlFlow::Continue(())
            }
            Err(_) => ControlFlow::Break(Err(GameError::IllegalMove {
                ply,
                mv: san_plus.to_string(),
            })),
        }
    }

    fn end_game(&mut self, movetext: Movetext) -> Self::Output {
        match (movetext.error, movetext.builder) {
            (Some(error), _) => Err(error),
            (None, Some(builder)) => Ok(builder.finish()),
            (None, None) => Err(GameError::Empty),
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p chess-analyzer-core --lib game::`
Expected: `16 passed; 0 failed`.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/core
git commit -m "Add PGN parsing and the Game model"
```

---

### Task 4: The engine seam and the UCI driver (`engine`)

**Files:**
- Create: `crates/core/src/engine.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `eval::{Eval, Side, UciScore}`.
- Produces:
  - `enum EngineError { NotFound, Spawn(String), Timeout(Duration), Protocol(String), NoAnalysis(String), Io(String) }` (derives `PartialEq`)
  - `struct Limits { depth: u32, multipv: u32 }` (`Default` = depth 20, MultiPV 3; serde)
  - `struct AnalysisLine { rank: u32, eval: Eval, depth: u32, pv: Vec<String> }` (rank 1 is best; `pv` is UCI moves)
  - `struct PositionAnalysis { lines: Vec<AnalysisLine> }` (sorted by rank, never empty)
  - `trait Analyzer { fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError>; fn engine_id(&self) -> String; }`, also implemented for `Box<T: Analyzer + ?Sized>`
  - `fn side_to_move(fen: &str) -> Side`, `fn parse_info_line(line: &str, stm: Side) -> Option<AnalysisLine>`, `fn locate_stockfish(explicit: Option<&Path>) -> Option<PathBuf>` (explicit path, then `STOCKFISH_PATH`, then `engines/stockfish[.exe]` in the current directory or any parent)
  - `struct EngineConfig { path, threads, hash_mb, timeout }` with `EngineConfig::new(path)` (1 thread, 256 MB, 120 s per analysis; starting the process has its own 30 s limit)
  - `struct UciEngine` with `UciEngine::start(EngineConfig) -> Result<UciEngine, EngineError>`, implementing `Analyzer` (on a timeout or a dead process it restarts and retries once) and killing the child on `Drop`
  - `struct ScriptedAnalyzer` with `ScriptedAnalyzer::new(Vec<PositionAnalysis>)`, `pub calls: usize`; returns its answers in order, then `NoAnalysis`. Used by every later unit test.

The unit tests here cover parsing, lookup and the scripted double. `UciEngine` itself needs a real Stockfish, so it is exercised in Task 11.

- [ ] **Step 1: Write the tests**

Create `crates/core/src/engine.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_centipawn_line() {
        let line = "info depth 20 seldepth 28 multipv 2 score cp 34 nodes 1000 nps 5000 time 200 pv e2e4 e7e5 g1f3";
        let parsed = parse_info_line(line, Side::White).unwrap();
        assert_eq!(parsed.rank, 2);
        assert_eq!(parsed.depth, 20);
        assert_eq!(parsed.eval, Eval::Cp(34));
        assert_eq!(parsed.pv, ["e2e4", "e7e5", "g1f3"]);
    }

    #[test]
    fn black_to_move_flips_the_score() {
        let line = "info depth 12 multipv 1 score cp 40 pv e7e5";
        assert_eq!(
            parse_info_line(line, Side::Black).unwrap().eval,
            Eval::Cp(-40)
        );
    }

    #[test]
    fn parses_mate_scores() {
        let line = "info depth 18 multipv 1 score mate 3 pv d8h4";
        assert_eq!(
            parse_info_line(line, Side::White).unwrap().eval,
            Eval::Mate(3)
        );
        let line = "info depth 18 multipv 1 score mate -2 pv d8h4";
        assert_eq!(
            parse_info_line(line, Side::White).unwrap().eval,
            Eval::Mate(-2)
        );
    }

    #[test]
    fn multipv_defaults_to_one() {
        let line = "info depth 5 score cp 10 pv e2e4";
        assert_eq!(parse_info_line(line, Side::White).unwrap().rank, 1);
    }

    #[test]
    fn ignores_lines_without_score_or_pv() {
        assert!(
            parse_info_line("info string NNUE evaluation using nn.nnue", Side::White).is_none()
        );
        assert!(
            parse_info_line("info depth 3 currmove e2e4 currmovenumber 1", Side::White).is_none()
        );
        assert!(parse_info_line("bestmove e2e4 ponder e7e5", Side::White).is_none());
        assert!(parse_info_line("", Side::White).is_none());
    }

    #[test]
    fn ignores_bound_scores() {
        let line = "info depth 10 multipv 1 score cp 80 lowerbound nodes 5 pv e2e4";
        assert!(parse_info_line(line, Side::White).is_none());
        let line = "info depth 10 multipv 1 score cp 80 upperbound nodes 5 pv e2e4";
        assert!(parse_info_line(line, Side::White).is_none());
    }

    #[test]
    fn side_to_move_is_read_from_the_fen() {
        assert_eq!(
            side_to_move("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
            Side::White
        );
        assert_eq!(
            side_to_move("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"),
            Side::Black
        );
    }

    #[test]
    fn missing_binary_is_reported() {
        let config = EngineConfig::new(PathBuf::from("definitely/not/here/stockfish"));
        assert!(matches!(
            UciEngine::start(config),
            Err(EngineError::NotFound)
        ));
    }

    #[test]
    fn explicit_missing_path_is_not_located() {
        assert!(locate_stockfish(Some(Path::new("definitely/not/here"))).is_none());
    }

    #[test]
    fn scripted_analyzer_returns_responses_in_order_then_errors() {
        let line = AnalysisLine {
            rank: 1,
            eval: Eval::Cp(0),
            depth: 1,
            pv: vec!["e2e4".into()],
        };
        let mut a = ScriptedAnalyzer::new(vec![PositionAnalysis {
            lines: vec![line.clone()],
        }]);
        assert_eq!(a.analyze("fen", &Limits::default()).unwrap().lines[0], line);
        assert!(a.analyze("fen", &Limits::default()).is_err());
        assert_eq!(a.calls, 2);
    }
}
```

- [ ] **Step 2: Register the module and watch it fail**

Add `pub mod engine;` to `crates/core/src/lib.rs`.

Run: `cargo test -p chess-analyzer-core --lib engine::`
Expected: compile errors such as `cannot find function 'parse_info_line' in this scope`.

- [ ] **Step 3: Implement**

Put this at the top of `crates/core/src/engine.rs`, above the `#[cfg(test)]` line:

```rust
//! Stockfish integration over UCI.
//!
//! `Analyzer` is the seam the rest of the crate depends on. `UciEngine` is the real
//! implementation; `ScriptedAnalyzer` is a canned one for tests.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::eval::{Eval, Side, UciScore};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("Stockfish was not found; run scripts/setup-stockfish or set the engine path")]
    NotFound,
    #[error("could not start the engine: {0}")]
    Spawn(String),
    #[error("the engine did not answer within {0:?}")]
    Timeout(Duration),
    #[error("unexpected engine output: {0}")]
    Protocol(String),
    #[error("the engine returned no analysis for {0}")]
    NoAnalysis(String),
    #[error("engine I/O failed: {0}")]
    Io(String),
}

/// Search limits for one analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub depth: u32,
    pub multipv: u32,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            depth: 20,
            multipv: 3,
        }
    }
}

/// One principal variation reported by the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisLine {
    /// 1 = best line.
    pub rank: u32,
    pub eval: Eval,
    pub depth: u32,
    /// Moves in UCI notation. The first move is the line's move.
    pub pv: Vec<String>,
}

/// The engine's analysis of one position; `lines` is sorted by rank and never empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionAnalysis {
    pub lines: Vec<AnalysisLine>,
}

pub trait Analyzer {
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError>;
    /// Identifies the engine and version; part of the cache key.
    fn engine_id(&self) -> String;
}

impl<T: Analyzer + ?Sized> Analyzer for Box<T> {
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        (**self).analyze(fen, limits)
    }

    fn engine_id(&self) -> String {
        (**self).engine_id()
    }
}

pub fn side_to_move(fen: &str) -> Side {
    if fen.split(' ').nth(1) == Some("b") {
        Side::Black
    } else {
        Side::White
    }
}

/// Parses a UCI `info` line into an analysis line (White's point of view).
/// Returns `None` for lines without a score and principal variation, and for
/// bound-only scores (`lowerbound` / `upperbound`).
pub fn parse_info_line(line: &str, stm: Side) -> Option<AnalysisLine> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "info" {
        return None;
    }
    let tokens: Vec<&str> = tokens.collect();
    let mut depth = None;
    let mut rank = 1;
    let mut score = None;
    let mut pv = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                depth = tokens.get(i + 1)?.parse::<u32>().ok();
                i += 2;
            }
            "multipv" => {
                rank = tokens.get(i + 1)?.parse::<u32>().ok()?;
                i += 2;
            }
            "score" => {
                let kind = *tokens.get(i + 1)?;
                let value = tokens.get(i + 2)?.parse::<i32>().ok()?;
                score = match kind {
                    "cp" => Some(UciScore::Cp(value)),
                    "mate" => Some(UciScore::Mate(value)),
                    _ => return None,
                };
                i += 3;
                if matches!(tokens.get(i), Some(&"lowerbound") | Some(&"upperbound")) {
                    return None;
                }
            }
            "pv" => {
                pv = tokens[i + 1..].iter().map(|s| s.to_string()).collect();
                break;
            }
            _ => i += 1,
        }
    }
    Some(AnalysisLine {
        rank,
        eval: Eval::from_uci(score?, stm),
        depth: depth?,
        pv,
    })
    .filter(|l| !l.pv.is_empty())
}

/// Finds a Stockfish executable: the explicit path, then `STOCKFISH_PATH`, then
/// `engines/stockfish[.exe]` in the current directory or any parent.
pub fn locate_stockfish(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return path.is_file().then(|| path.to_path_buf());
    }
    if let Ok(path) = std::env::var("STOCKFISH_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let name = if cfg!(windows) {
        "stockfish.exe"
    } else {
        "stockfish"
    };
    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let candidate = d.join("engines").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

/// How long the engine gets to start and answer the UCI handshake.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub path: PathBuf,
    pub threads: u32,
    pub hash_mb: u32,
    /// Maximum time to wait for any single analysis (starting the process has its own, longer limit).
    pub timeout: Duration,
}

impl EngineConfig {
    pub fn new(path: PathBuf) -> EngineConfig {
        EngineConfig {
            path,
            threads: 1,
            hash_mb: 256,
            timeout: Duration::from_secs(120),
        }
    }
}

/// A running Stockfish process.
pub struct UciEngine {
    config: EngineConfig,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    name: String,
}

impl UciEngine {
    pub fn start(config: EngineConfig) -> Result<UciEngine, EngineError> {
        if !config.path.is_file() {
            return Err(EngineError::NotFound);
        }
        let mut child = Command::new(&config.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| EngineError::Spawn(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EngineError::Spawn("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Spawn("no stdout".into()))?;
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut engine = UciEngine {
            config,
            child,
            stdin,
            lines,
            name: String::new(),
        };
        engine.handshake()?;
        Ok(engine)
    }

    fn handshake(&mut self) -> Result<(), EngineError> {
        self.send("uci")?;
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            let line = self.next_line(deadline, STARTUP_TIMEOUT)?;
            if let Some(name) = line.strip_prefix("id name ") {
                self.name = name.trim().to_string();
            }
            if line.trim() == "uciok" {
                break;
            }
        }
        self.send(&format!(
            "setoption name Threads value {}",
            self.config.threads
        ))?;
        self.send(&format!(
            "setoption name Hash value {}",
            self.config.hash_mb
        ))?;
        self.send("isready")?;
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while self.next_line(deadline, STARTUP_TIMEOUT)?.trim() != "readyok" {}
        Ok(())
    }

    fn send(&mut self, command: &str) -> Result<(), EngineError> {
        writeln!(self.stdin, "{command}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| EngineError::Io(e.to_string()))
    }

    fn next_line(&mut self, deadline: Instant, limit: Duration) -> Result<String, EngineError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match self.lines.recv_timeout(remaining) {
            Ok(line) => Ok(line),
            Err(RecvTimeoutError::Timeout) => Err(EngineError::Timeout(limit)),
            Err(RecvTimeoutError::Disconnected) => Err(EngineError::Io("engine exited".into())),
        }
    }

    fn try_analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        let stm = side_to_move(fen);
        self.send(&format!("setoption name MultiPV value {}", limits.multipv))?;
        self.send(&format!("position fen {fen}"))?;
        self.send(&format!("go depth {}", limits.depth))?;
        let deadline = Instant::now() + self.config.timeout;
        let mut best: BTreeMap<u32, AnalysisLine> = BTreeMap::new();
        loop {
            let line = self.next_line(deadline, self.config.timeout)?;
            if line.starts_with("bestmove") {
                break;
            }
            if let Some(parsed) = parse_info_line(&line, stm) {
                best.insert(parsed.rank, parsed);
            }
        }
        if best.is_empty() {
            return Err(EngineError::NoAnalysis(fen.to_string()));
        }
        Ok(PositionAnalysis {
            lines: best.into_values().collect(),
        })
    }

    fn restart(&mut self) -> Result<(), EngineError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let fresh = UciEngine::start(self.config.clone())?;
        *self = fresh;
        Ok(())
    }
}

impl Analyzer for UciEngine {
    /// Analyses a position. On a timeout or a dead process the engine is restarted
    /// and the position retried once.
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        match self.try_analyze(fen, limits) {
            Err(EngineError::Timeout(_) | EngineError::Io(_)) => {
                self.restart()?;
                self.try_analyze(fen, limits)
            }
            other => other,
        }
    }

    fn engine_id(&self) -> String {
        self.name.clone()
    }
}

impl Drop for UciEngine {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// An `Analyzer` that returns canned answers in order. For tests.
pub struct ScriptedAnalyzer {
    responses: VecDeque<PositionAnalysis>,
    pub calls: usize,
}

impl ScriptedAnalyzer {
    pub fn new(responses: Vec<PositionAnalysis>) -> ScriptedAnalyzer {
        ScriptedAnalyzer {
            responses: responses.into(),
            calls: 0,
        }
    }
}

impl Analyzer for ScriptedAnalyzer {
    fn analyze(&mut self, fen: &str, _limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        self.calls += 1;
        self.responses
            .pop_front()
            .ok_or_else(|| EngineError::NoAnalysis(fen.to_string()))
    }

    fn engine_id(&self) -> String {
        "scripted".to_string()
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p chess-analyzer-core --lib engine::`
Expected: `10 passed; 0 failed`.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/core
git commit -m "Add the Analyzer seam, UCI parsing and the Stockfish driver"
```

---

### Task 5: Stockfish setup scripts

**Files:**
- Create: `scripts/setup-stockfish.ps1`, `scripts/setup-stockfish.sh`

**Interfaces:**
- Produces: `engines/stockfish.exe` (Windows) or `engines/stockfish` (Linux/macOS), found automatically by `locate_stockfish`.

Notes from checking the release `sf_19`: the "universal" archives contain a `stockfish/` folder with the full source tree and one executable named like the archive (`stockfish-windows-x86-64-universal.exe`, 103 MB, `stockfish-linux-x86-64-universal`, `stockfish-macos-universal`), so the scripts search for the executable and skip `src/` and `scripts/`. The PowerShell script sets the console input encoding to UTF-8 *without* a BOM before talking to the engine: with a BOM Stockfish sees `﻿uci` and answers `Unknown command`.

- [ ] **Step 1: Create the PowerShell script**

`scripts/setup-stockfish.ps1`:

```powershell
﻿# Downloads Stockfish into engines\stockfish.exe (gitignored).
# Usage: scripts\setup-stockfish.ps1 [-Version sf_19] [-Destination <dir>]
param(
    [string]$Version = "sf_19",
    [string]$Destination = (Join-Path $PSScriptRoot "..\engines")
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # the progress bar makes Invoke-WebRequest very slow

$arch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq "Arm64") { "arm64" } else { "x86-64" }
$asset = "stockfish-windows-$arch-universal.zip"
$url = "https://github.com/official-stockfish/Stockfish/releases/download/$Version/$asset"

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("stockfish-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Join-Path $work $asset
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zip

    $extracted = Join-Path $work "extracted"
    Expand-Archive -Path $zip -DestinationPath $extracted

    $exe = Get-ChildItem -Path $extracted -Recurse -File -Filter "stockfish*.exe" | Select-Object -First 1
    if (-not $exe) { throw "No Stockfish executable found inside $asset" }

    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $target = Join-Path (Resolve-Path $Destination) "stockfish.exe"
    Copy-Item -Path $exe.FullName -Destination $target -Force

    # A UTF-8 console would prepend a byte-order mark to what we send ("﻿uci" is not a UCI command),
    # so switch the console input encoding to UTF-8 without a BOM before starting the process.
    [Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $target
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $process = [System.Diagnostics.Process]::Start($info)
    $process.StandardInput.WriteLine("uci")
    $process.StandardInput.WriteLine("quit")
    $reply = $process.StandardOutput.ReadToEnd()
    $process.WaitForExit()
    if ($reply -notmatch "uciok") { throw "$target did not answer the UCI handshake" }
    $name = ($reply -split "`r?`n" | Where-Object { $_ -like "id name*" } | Select-Object -First 1)
    Write-Host "Installed $name at $target"
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
```

- [ ] **Step 2: Create the bash script**

`scripts/setup-stockfish.sh`:

```bash
#!/usr/bin/env bash
# Downloads Stockfish into engines/stockfish (gitignored). Linux and macOS.
# Usage: scripts/setup-stockfish.sh [version] [destination-dir]
set -euo pipefail

VERSION="${1:-sf_19}"
DEST="${2:-$(cd "$(dirname "$0")/.." && pwd)/engines}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  ASSET="stockfish-linux-x86-64-universal.tar.gz" ;;
  Linux-aarch64) ASSET="stockfish-linux-arm64-universal.tar.gz" ;;
  Darwin-*)      ASSET="stockfish-macos-universal.tar.gz" ;;
  *) echo "Unsupported platform: $(uname -s)-$(uname -m). Download Stockfish manually and set STOCKFISH_PATH." >&2; exit 1 ;;
esac

URL="https://github.com/official-stockfish/Stockfish/releases/download/${VERSION}/${ASSET}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "Downloading $URL"
curl -fL --retry 3 -o "$WORK/$ASSET" "$URL"
mkdir "$WORK/extracted"
tar -xzf "$WORK/$ASSET" -C "$WORK/extracted"

BIN="$(find "$WORK/extracted" -type f -name 'stockfish*' -perm -u+x | grep -Ev '/src/|/scripts/' | head -n 1)"
if [ -z "$BIN" ]; then
  echo "No Stockfish executable found inside $ASSET" >&2
  exit 1
fi

mkdir -p "$DEST"
cp "$BIN" "$DEST/stockfish"
chmod +x "$DEST/stockfish"

REPLY="$(printf 'uci\nquit\n' | "$DEST/stockfish")"
case "$REPLY" in
  *uciok*) echo "Installed $(printf '%s\n' "$REPLY" | grep 'id name') at $DEST/stockfish" ;;
  *) echo "$DEST/stockfish did not answer the UCI handshake" >&2; exit 1 ;;
esac
```

- [ ] **Step 3: Install Stockfish (Windows)**

Run (PowerShell):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/setup-stockfish.ps1
```

Expected: `Downloading https://github.com/official-stockfish/Stockfish/releases/download/sf_19/stockfish-windows-x86-64-universal.zip`, then `Installed id name Stockfish 19 at ...\engines\stockfish.exe`, and `engines/stockfish.exe` exists (about 100 MB). It stays out of git because `/engines/` is ignored.

On Linux or macOS run `bash scripts/setup-stockfish.sh` instead and expect the same `Installed id name Stockfish 19 at .../engines/stockfish`. (When this plan was written the bash script was only syntax-checked with `bash -n` and the archive layout inspected, not run on Linux or macOS; if it fails there, fix the script and mention it in the commit message.)

- [ ] **Step 4: Confirm the repo ignores the binary**

Run: `git status --short`
Expected: lists `scripts/` as untracked, and does not list `engines/`.

- [ ] **Step 5: Commit**

```bash
git add scripts
git commit -m "Add Stockfish setup scripts"
```

---

### Task 6: Move classification (`classify`)

**Files:**
- Create: `crates/core/src/classify.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `eval::{Eval, Side}`.
- Produces:
  - `enum MoveClass { Book, Brilliant, Great, Best, Good, Inaccuracy, Mistake, Miss, Blunder }` (serde snake_case; `is_critical()` is true for Brilliant, Great, Inaccuracy, Mistake, Miss, Blunder)
  - `struct Thresholds` (serde; `Default`): `best_epsilon 0.5`, `book_max_loss 10.0`, `good_max 5.0`, `inaccuracy_max 10.0`, `mistake_max 20.0`, `great_gap 12.0`, `decided_win 97.0`, `brilliant_max_loss 2.0`, `brilliant_min_sacrifice 2`, `brilliant_min_win_after 50.0`, `brilliant_max_win_before 90.0`, `miss_min_win_before 60.0`. Win-percentage loss is in percentage points, from the mover's point of view.
  - `struct MoveContext { mover: Side, played_uci: String, best_uci: Option<String>, win_before: f64, win_second: Option<f64>, win_after: f64, eval_before: Eval, eval_after: Eval, in_book: bool, prev_opponent_class: Option<MoveClass>, material_swing: i32 }` with `fn loss(&self) -> f64` (0 when the played move is the engine's best move)
  - `fn classify(ctx: &MoveContext, t: &Thresholds) -> MoveClass`

Rule order in `classify`: Book (only if the loss is at most `book_max_loss`) → walking into a forced mate is a Blunder → dropping a forced mate is a Miss if the mover is still at least `miss_min_win_before` afterwards, otherwise a Blunder → Brilliant (sound sacrifice) → Best or Great (best move whose second-best line is at least `great_gap` worse, in an undecided position) → Good / Inaccuracy / Mistake / Blunder by loss, where an Inaccuracy or Mistake becomes a Miss when the opponent just made a Mistake, Miss or Blunder and the mover had at least `miss_min_win_before`.

- [ ] **Step 1: Write the tests**

Create `crates/core/src/classify.rs` containing only:

```rust
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
```

- [ ] **Step 2: Register the module and watch it fail**

Add `pub mod classify;` to `crates/core/src/lib.rs` (alphabetical: it goes before `engine`).

Run: `cargo test -p chess-analyzer-core --lib classify::`
Expected: compile errors such as `cannot find type 'MoveContext' in this scope`.

- [ ] **Step 3: Implement**

Put this at the top of `crates/core/src/classify.rs`, above the `#[cfg(test)]` line:

```rust
//! Move classification. Pure functions over numbers; no engine or board access.
//!
//! Win percentages are from the mover's point of view, in 0..=100.

use serde::{Deserialize, Serialize};

use crate::eval::{Eval, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p chess-analyzer-core --lib classify::`
Expected: `20 passed; 0 failed`.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/core
git commit -m "Add move classification"
```

---

### Task 7: Opening names and book positions (`openings`)

**Files:**
- Create: `data/openings.tsv`, `data/README.md`, `crates/core/src/openings.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `game::parse_pgn`.
- Produces:
  - `struct Opening { eco: String, name: String }` (serde)
  - `enum OpeningsError { BadLine { line: usize, reason: String } }`
  - `struct OpeningBook` with `OpeningBook::empty()`, `OpeningBook::from_tsv(&str) -> Result<OpeningBook, OpeningsError>` (rows `eco<TAB>name<TAB>pgn`, first row is a header), `OpeningBook::bundled() -> &'static OpeningBook` (the compiled-in `data/openings.tsv`), `is_book(&self, fen: &str) -> bool` (any position along any known line), `name_of(&self, fen: &str) -> Option<&Opening>` (only the end of a named line)
  - Positions are matched on the first four FEN fields (board, side to move, castling, en passant), so transpositions and move counters do not matter.

- [ ] **Step 1: Fetch the opening data**

Create `data/README.md`:

````markdown
# data

- `openings.tsv`: opening names and book lines, concatenated from the `a.tsv` to `e.tsv` files of
  [lichess-org/chess-openings](https://github.com/lichess-org/chess-openings) (CC0-1.0, public domain).
  Columns: `eco`, `name`, `pgn`. To refresh it:

  ```bash
  { printf 'eco\tname\tpgn\n'; for f in a b c d e; do
      curl -sfL "https://raw.githubusercontent.com/lichess-org/chess-openings/master/$f.tsv" | tail -n +2
    done; } | tr -d '\r' > data/openings.tsv
  ```

- `fixtures/`: small games used by the golden tests. `<name>.pgn` is the game,
  `<name>.analysis.json` is Stockfish's recorded analysis of it, and `<name>.golden.txt` is the
  expected review summary. See `crates/core/tests/golden.rs` for how to re-record and update them.
````

Then build `data/openings.tsv`:

```bash
mkdir -p data
{ printf 'eco\tname\tpgn\n'; for f in a b c d e; do
    curl -sfL "https://raw.githubusercontent.com/lichess-org/chess-openings/master/$f.tsv" | tail -n +2
  done; } | tr -d '\r' > data/openings.tsv
wc -l data/openings.tsv
head -3 data/openings.tsv
```

Expected: about 3,866 lines (the dataset grows over time, so treat the exact count as approximate) and a first line `eco	name	pgn` followed by `A00	Amar Opening	1. Nh3`.

- [ ] **Step 2: Write the tests**

Create `crates/core/src/openings.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const TSV: &str = "eco\tname\tpgn\n\
        B00\tKing's Pawn\t1. e4\n\
        C20\tKing's Pawn Game\t1. e4 e5\n\
        C44\tKing's Knight Opening\t1. e4 e5 2. Nf3\n";

    fn fens(pgn: &str) -> Vec<String> {
        parse_pgn(pgn).unwrap()[0].positions.clone()
    }

    #[test]
    fn every_position_along_a_line_is_book() {
        let book = OpeningBook::from_tsv(TSV).unwrap();
        for fen in fens("1. e4 e5 2. Nf3") {
            assert!(book.is_book(&fen), "{fen}");
        }
        assert!(!book.is_book(&fens("1. e4 e5 2. Nf3 Nc6")[4]));
    }

    #[test]
    fn names_are_attached_to_the_end_of_a_line() {
        let book = OpeningBook::from_tsv(TSV).unwrap();
        let positions = fens("1. e4 e5 2. Nf3");
        assert_eq!(book.name_of(&positions[1]).unwrap().name, "King's Pawn");
        assert_eq!(book.name_of(&positions[3]).unwrap().eco, "C44");
        assert!(book.name_of(&positions[0]).is_none());
    }

    #[test]
    fn move_counters_do_not_matter() {
        let book = OpeningBook::from_tsv(TSV).unwrap();
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 17 99";
        assert!(book.is_book(fen));
    }

    #[test]
    fn transpositions_are_recognised() {
        let tsv = "eco\tname\tpgn\nD00\tQueen's Pawn\t1. d4 d5 2. Nf3\n";
        let book = OpeningBook::from_tsv(tsv).unwrap();
        let transposed = fens("1. Nf3 d5 2. d4");
        assert!(book.is_book(&transposed[3]));
        assert_eq!(book.name_of(&transposed[3]).unwrap().name, "Queen's Pawn");
    }

    #[test]
    fn malformed_rows_are_reported_with_their_line() {
        let err = OpeningBook::from_tsv("eco\tname\tpgn\nA00\tonly two columns\n").unwrap_err();
        assert!(matches!(err, OpeningsError::BadLine { line: 2, .. }));
        let err = OpeningBook::from_tsv("eco\tname\tpgn\nA00\tBad\t1. e4 e4\n").unwrap_err();
        assert!(matches!(err, OpeningsError::BadLine { line: 2, .. }));
    }

    #[test]
    fn bundled_dataset_loads_and_knows_the_ruy_lopez() {
        let book = OpeningBook::bundled();
        let positions = fens("1. e4 e5 2. Nf3 Nc6 3. Bb5");
        let opening = book.name_of(&positions[5]).expect("Ruy Lopez is named");
        assert!(opening.name.starts_with("Ruy Lopez"), "{}", opening.name);
        assert!(opening.eco.starts_with('C'));
    }
}
```

- [ ] **Step 3: Register the module and watch it fail**

Add `pub mod openings;` to `crates/core/src/lib.rs`.

Run: `cargo test -p chess-analyzer-core --lib openings::`
Expected: compile errors such as `cannot find type 'OpeningBook' in this scope`.

- [ ] **Step 4: Implement**

Put this at the top of `crates/core/src/openings.rs`, above the `#[cfg(test)]` line:

```rust
//! Opening names and book positions, from the lichess-org/chess-openings dataset (CC0).
//!
//! Positions are matched by FEN (board, side to move, castling, en passant), so
//! transpositions are recognised.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::game::parse_pgn;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opening {
    pub eco: String,
    pub name: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OpeningsError {
    #[error("openings line {line}: {reason}")]
    BadLine { line: usize, reason: String },
}

#[derive(Debug, Default)]
pub struct OpeningBook {
    /// Every position that occurs along any known line.
    book: HashSet<String>,
    /// Positions at the end of a named line.
    names: HashMap<String, Opening>,
}

/// The first four FEN fields: everything except the move counters.
fn key(fen: &str) -> String {
    fen.split(' ').take(4).collect::<Vec<_>>().join(" ")
}

impl OpeningBook {
    /// A book that recognises nothing.
    pub fn empty() -> OpeningBook {
        OpeningBook::default()
    }

    /// Parses tab-separated `eco<TAB>name<TAB>pgn` rows. The first row is a header.
    pub fn from_tsv(text: &str) -> Result<OpeningBook, OpeningsError> {
        let mut book = OpeningBook::default();
        for (i, row) in text.lines().enumerate().skip(1) {
            if row.trim().is_empty() {
                continue;
            }
            let line = i + 1;
            let mut cols = row.split('\t');
            let (Some(eco), Some(name), Some(pgn)) = (cols.next(), cols.next(), cols.next()) else {
                return Err(OpeningsError::BadLine {
                    line,
                    reason: "expected 3 columns".into(),
                });
            };
            let games = parse_pgn(pgn).map_err(|e| OpeningsError::BadLine {
                line,
                reason: e.to_string(),
            })?;
            let game = &games[0];
            for fen in &game.positions {
                book.book.insert(key(fen));
            }
            let last = key(game.positions.last().expect("at least the start position"));
            book.names.entry(last).or_insert_with(|| Opening {
                eco: eco.to_string(),
                name: name.to_string(),
            });
        }
        Ok(book)
    }

    /// The dataset bundled into the binary (`data/openings.tsv`).
    pub fn bundled() -> &'static OpeningBook {
        static BOOK: OnceLock<OpeningBook> = OnceLock::new();
        BOOK.get_or_init(|| {
            OpeningBook::from_tsv(include_str!("../../../data/openings.tsv"))
                .expect("bundled openings.tsv is valid")
        })
    }

    pub fn is_book(&self, fen: &str) -> bool {
        self.book.contains(&key(fen))
    }

    pub fn name_of(&self, fen: &str) -> Option<&Opening> {
        self.names.get(&key(fen))
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p chess-analyzer-core --lib openings::`
Expected: `6 passed; 0 failed`. The last test, `bundled_dataset_loads_and_knows_the_ruy_lopez`, proves every row of the real dataset parses.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add data crates/core
git commit -m "Add opening names and book positions"
```

---

### Task 8: The review pipeline (`review`)

**Files:**
- Create: `crates/core/src/review.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: everything from Tasks 2 to 7 (`Game`, `Analyzer`, `PositionAnalysis`, `AnalysisLine`, `Limits`, `EngineError`, `MoveClass`, `MoveContext`, `Thresholds`, `classify`, `OpeningBook`, `Opening`, `Eval`, `Side`, `move_accuracy`; tests also use `ScriptedAnalyzer`).
- Produces:
  - `struct ReviewOptions { limits: Limits, thresholds: Thresholds }` (`Default`, `Copy`)
  - `struct Progress { done: usize, total: usize }`
  - `enum ReviewError { Engine(EngineError), Cancelled }`
  - `struct MoveReview { ply, move_number, side, san, uci, class, eval_before, eval_after, best_uci, best_san, best_pv, loss, accuracy, critical }` (serde)
  - `struct Accuracy { white: Option<f64>, black: Option<f64> }`
  - `struct Review { headers, opening, engine, limits, evals, moves, accuracy, critical_plies }` (serde). `evals.len() == moves.len() + 1`; `critical_plies` are 1-based.
  - `fn review_game(game: &Game, analyzer: &mut dyn Analyzer, options: &ReviewOptions, book: &OpeningBook, cancel: &AtomicBool, on_progress: impl FnMut(Progress)) -> Result<Review, ReviewError>`

How it works: it analyses positions `0..=n` in order. A finished position (checkmate, stalemate, insufficient material) is scored locally and never sent to the engine. For each move: the best line of the position before gives `eval_before` and the engine's move; `eval_after` comes from the line matching the played move if it is among the MultiPV lines, otherwise from the next position's best line. `material_swing` is the mover's material balance change after the opponent's best reply (so a sacrificed piece that is not recaptured shows as negative). Book plies are the unbroken run from the start whose resulting position is in the book. Per-player accuracy is the mean of that player's per-move accuracies.

- [ ] **Step 1: Write the tests**

Create `crates/core/src/review.rs` containing only:

```rust
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
}
```

- [ ] **Step 2: Register the module and watch it fail**

Add `pub mod review;` to `crates/core/src/lib.rs`.

Run: `cargo test -p chess-analyzer-core --lib review::`
Expected: compile errors such as `cannot find function 'review_game' in this scope`.

- [ ] **Step 3: Implement**

Put this at the top of `crates/core/src/review.rs`, above the `#[cfg(test)]` line:

```rust
//! The review pipeline: a `Game` plus an `Analyzer` becomes a `Review`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{Chess, Color, Position, Role};
use thiserror::Error;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Accuracy {
    pub white: Option<f64>,
    pub black: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

fn balance(pos: &Chess, color: Color) -> i32 {
    material(pos, color) - material(pos, color.other())
}

fn apply_uci(pos: &Chess, uci: &str) -> Option<Chess> {
    let mv = UciMove::from_ascii(uci.as_bytes())
        .ok()?
        .to_move(pos)
        .ok()?;
    let mut next = pos.clone();
    next.play_unchecked(mv);
    Some(next)
}

fn uci_to_san(pos: &Chess, uci: &str) -> Option<String> {
    let mv = UciMove::from_ascii(uci.as_bytes())
        .ok()?
        .to_move(pos)
        .ok()?;
    Some(SanPlus::from_move(pos.clone(), mv).to_string())
}

/// What the engine would say about a finished game, without asking it.
fn terminal_analysis(pos: &Chess) -> Option<PositionAnalysis> {
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

pub fn review_game(
    game: &Game,
    analyzer: &mut dyn Analyzer,
    options: &ReviewOptions,
    book: &OpeningBook,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<Review, ReviewError> {
    let n = game.moves.len();
    let total = n + 1;

    let mut analyses: Vec<PositionAnalysis> = Vec::with_capacity(total);
    for i in 0..total {
        if cancel.load(Ordering::Relaxed) {
            return Err(ReviewError::Cancelled);
        }
        let pos = game.position(i);
        let analysis = match terminal_analysis(&pos) {
            Some(terminal) => terminal,
            None => analyzer.analyze(&game.positions[i], &options.limits)?,
        };
        analyses.push(analysis);
        on_progress(Progress { done: i + 1, total });
    }

    // Book: the unbroken run of plies whose resulting position is a known opening position.
    let mut book_plies = 0;
    let mut opening = None;
    for ply in 1..=n {
        if !book.is_book(&game.positions[ply]) {
            break;
        }
        book_plies = ply;
        if let Some(named) = book.name_of(&game.positions[ply]) {
            opening = Some(named.clone());
        }
    }

    let mut moves: Vec<MoveReview> = Vec::with_capacity(n);
    for i in 0..n {
        let before = game.position(i);
        let after = game.position(i + 1);
        let mover = Side::from(before.turn());
        let played = &game.moves[i];
        let lines = &analyses[i].lines;
        let best = &lines[0];

        let eval_after = lines
            .iter()
            .find(|l| l.pv.first() == Some(&played.uci))
            .map(|l| l.eval)
            .unwrap_or(analyses[i + 1].lines[0].eval);

        let material_swing = {
            let start = balance(&before, before.turn());
            let reply = analyses[i + 1].lines[0].pv.first();
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
            prev_opponent_class: moves.last().map(|m: &MoveReview| m.class),
            material_swing,
        };
        let class = classify(&ctx, &options.thresholds);
        let loss = ctx.loss();

        moves.push(MoveReview {
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
        });
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
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p chess-analyzer-core --lib review::`
Expected: `11 passed; 0 failed`.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/core
git commit -m "Add the review pipeline"
```

---

### Task 9: The SQLite analysis cache (`cache`)

**Files:**
- Create: `crates/core/src/cache.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `engine::{Analyzer, EngineError, Limits, PositionAnalysis}` (tests also use `AnalysisLine`, `ScriptedAnalyzer`, `Eval`).
- Produces:
  - `struct CacheError` (Display: `analysis cache unavailable: ...`)
  - `fn open_database(path: &Path) -> Result<rusqlite::Connection, CacheError>` (creates the table; fails on an unwritable path or a file that is not a database), `fn in_memory_database() -> Result<Connection, CacheError>`
  - `struct CachedAnalyzer<A: Analyzer>` with `new(inner, conn)`, `open(inner, path) -> Result<..>`, `in_memory(inner) -> Result<..>`, `pub hits: usize`, `pub misses: usize`; implements `Analyzer`
  - Cache key: first four FEN fields + engine id (`engine_id()`) + depth + MultiPV. Engine errors are not cached; write failures never fail an analysis.

- [ ] **Step 1: Write the tests**

Create `crates/core/src/cache.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AnalysisLine, ScriptedAnalyzer};
    use crate::eval::Eval;

    fn analysis(cp: i32) -> PositionAnalysis {
        PositionAnalysis {
            lines: vec![AnalysisLine {
                rank: 1,
                eval: Eval::Cp(cp),
                depth: 20,
                pv: vec!["e2e4".into()],
            }],
        }
    }

    #[test]
    fn second_lookup_is_served_from_the_cache() {
        let scripted = ScriptedAnalyzer::new(vec![analysis(10)]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        let limits = Limits::default();
        let first = cached.analyze("fen-a", &limits).unwrap();
        let second = cached.analyze("fen-a", &limits).unwrap();
        assert_eq!(first, second);
        assert_eq!((cached.hits, cached.misses), (1, 1));
    }

    #[test]
    fn different_limits_or_positions_miss() {
        let scripted = ScriptedAnalyzer::new(vec![analysis(1), analysis(2), analysis(3)]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        cached
            .analyze(
                "fen-a",
                &Limits {
                    depth: 10,
                    multipv: 1,
                },
            )
            .unwrap();
        cached
            .analyze(
                "fen-a",
                &Limits {
                    depth: 12,
                    multipv: 1,
                },
            )
            .unwrap();
        cached
            .analyze(
                "fen-b",
                &Limits {
                    depth: 10,
                    multipv: 1,
                },
            )
            .unwrap();
        assert_eq!((cached.hits, cached.misses), (0, 3));
    }

    #[test]
    fn positions_that_differ_only_in_move_counters_share_an_entry() {
        let scripted = ScriptedAnalyzer::new(vec![analysis(5)]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        let limits = Limits::default();
        let a = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
        let b = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 7 12";
        cached.analyze(a, &limits).unwrap();
        cached.analyze(b, &limits).unwrap();
        assert_eq!((cached.hits, cached.misses), (1, 1));
    }

    #[test]
    fn a_different_side_to_move_is_a_different_position() {
        let scripted = ScriptedAnalyzer::new(vec![analysis(5), analysis(6)]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        let limits = Limits::default();
        cached
            .analyze("8/8/8/8/8/8/8/K1k5 w - - 0 1", &limits)
            .unwrap();
        cached
            .analyze("8/8/8/8/8/8/8/K1k5 b - - 0 1", &limits)
            .unwrap();
        assert_eq!((cached.hits, cached.misses), (0, 2));
    }
    #[test]
    fn engine_errors_are_not_cached() {
        let scripted = ScriptedAnalyzer::new(vec![]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        assert!(cached.analyze("fen-a", &Limits::default()).is_err());
        assert_eq!(cached.hits, 0);
    }

    #[test]
    fn the_cache_survives_reopening_the_file() {
        let dir =
            std::env::temp_dir().join(format!("chess-analyzer-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.db");
        let _ = std::fs::remove_file(&path);

        let mut first =
            CachedAnalyzer::open(ScriptedAnalyzer::new(vec![analysis(7)]), &path).unwrap();
        first.analyze("fen-a", &Limits::default()).unwrap();
        drop(first);

        let mut second = CachedAnalyzer::open(ScriptedAnalyzer::new(vec![]), &path).unwrap();
        let hit = second.analyze("fen-a", &Limits::default()).unwrap();
        assert_eq!(hit, analysis(7));
        assert_eq!((second.hits, second.misses), (1, 0));
        drop(second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_is_reported() {
        let dir = std::env::temp_dir().join(format!(
            "chess-analyzer-corrupt-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.db");
        std::fs::write(
            &path,
            b"this is not a sqlite database, just some text padding it out to be long enough",
        )
        .unwrap();
        assert!(open_database(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unwritable_path_is_reported() {
        let result = CachedAnalyzer::open(
            ScriptedAnalyzer::new(vec![]),
            Path::new("definitely/not/a/real/dir/cache.db"),
        );
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Register the module and watch it fail**

Add `pub mod cache;` to `crates/core/src/lib.rs` (alphabetical: first).

Run: `cargo test -p chess-analyzer-core --lib cache::`
Expected: compile errors such as `cannot find type 'CachedAnalyzer' in this scope`.

- [ ] **Step 3: Implement**

Put this at the top of `crates/core/src/cache.rs`, above the `#[cfg(test)]` line:

```rust
//! SQLite cache of engine analyses, as a decorator around any `Analyzer`.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::engine::{Analyzer, EngineError, Limits, PositionAnalysis};

#[derive(Debug, Error)]
#[error("analysis cache unavailable: {0}")]
pub struct CacheError(String);

impl From<rusqlite::Error> for CacheError {
    fn from(e: rusqlite::Error) -> CacheError {
        CacheError(e.to_string())
    }
}

/// Looks positions up by (FEN, engine id, depth, MultiPV) before asking the wrapped analyzer.
pub struct CachedAnalyzer<A: Analyzer> {
    inner: A,
    conn: Connection,
    pub hits: usize,
    pub misses: usize,
}

/// Opens (creating it if needed) a cache database. Fails if the file cannot be opened,
/// is not a database, or cannot be written. Callers decide whether to carry on uncached.
pub fn open_database(path: &Path) -> Result<Connection, CacheError> {
    prepare(Connection::open(path)?)
}

pub fn in_memory_database() -> Result<Connection, CacheError> {
    prepare(Connection::open_in_memory()?)
}

fn prepare(conn: Connection) -> Result<Connection, CacheError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS analysis (
            fen      TEXT    NOT NULL,
            engine   TEXT    NOT NULL,
            depth    INTEGER NOT NULL,
            multipv  INTEGER NOT NULL,
            result   TEXT    NOT NULL,
            PRIMARY KEY (fen, engine, depth, multipv)
        )",
    )?;
    Ok(conn)
}

impl<A: Analyzer> CachedAnalyzer<A> {
    pub fn new(inner: A, conn: Connection) -> CachedAnalyzer<A> {
        CachedAnalyzer {
            inner,
            conn,
            hits: 0,
            misses: 0,
        }
    }

    pub fn open(inner: A, path: &Path) -> Result<CachedAnalyzer<A>, CacheError> {
        Ok(CachedAnalyzer::new(inner, open_database(path)?))
    }

    pub fn in_memory(inner: A) -> Result<CachedAnalyzer<A>, CacheError> {
        Ok(CachedAnalyzer::new(inner, in_memory_database()?))
    }

    fn lookup(&self, fen: &str, engine: &str, limits: &Limits) -> Option<PositionAnalysis> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT result FROM analysis WHERE fen = ?1 AND engine = ?2 AND depth = ?3 AND multipv = ?4",
                params![fen, engine, limits.depth, limits.multipv],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        json.and_then(|j| serde_json::from_str(&j).ok())
    }

    fn store(&self, fen: &str, engine: &str, limits: &Limits, analysis: &PositionAnalysis) {
        // A failed write only costs a future cache hit, so it never fails the review.
        if let Ok(json) = serde_json::to_string(analysis) {
            let _ = self.conn.execute(
                "INSERT OR REPLACE INTO analysis (fen, engine, depth, multipv, result) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![fen, engine, limits.depth, limits.multipv, json],
            );
        }
    }
}

/// The cache key for a position: board, side to move, castling and en passant. The move/// counters are dropped so the same position reached by a different move order shares an entry.fn position_key(fen: &str) -> String {    fen.split(' ').take(4).collect::<Vec<_>>().join(" ")}
/// The cache key for a position: board, side to move, castling and en passant. The move
/// counters are dropped so the same position reached by a different move order shares an entry.
fn position_key(fen: &str) -> String {
    fen.split(' ').take(4).collect::<Vec<_>>().join(" ")
}

impl<A: Analyzer> Analyzer for CachedAnalyzer<A> {
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        let engine = self.inner.engine_id();
        let key = position_key(fen);
        if let Some(hit) = self.lookup(&key, &engine, limits) {
            self.hits += 1;
            return Ok(hit);
        }
        self.misses += 1;
        let analysis = self.inner.analyze(fen, limits)?;
        self.store(&key, &engine, limits, &analysis);
        Ok(analysis)
    }

    fn engine_id(&self) -> String {
        self.inner.engine_id()
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p chess-analyzer-core --lib cache::`
Expected: `8 passed; 0 failed`.

- [ ] **Step 5: Run the whole library suite, then commit**

Run: `cargo test -p chess-analyzer-core --lib`
Expected: `82 passed; 0 failed`.

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/core
git commit -m "Add the SQLite analysis cache"
```

---

### Task 10: The command-line reviewer (`cli`)

**Files:**
- Create: `crates/cli/src/report.rs`
- Modify: `crates/cli/src/main.rs`

**Interfaces:**
- Consumes: `chess_analyzer_core::{cache, engine, game, openings, review, classify, eval}`.
- Produces: the `chess-analyzer review <pgn|-> [--game N] [--depth 20] [--multipv 3] [--threads 1] [--hash 256] [--engine PATH] [--cache FILE] [--no-cache] [--json]` command. Progress goes to stderr, the report (or JSON) to stdout. `report::render(&Review) -> String`.

Behaviour: a PGN with several games reviews game `--game` (default 1) and says so on stderr; an out-of-range `--game` is an error; a missing Stockfish gives an actionable message; a cache that cannot be opened prints a warning and continues without it.

- [ ] **Step 1: Write the report tests**

Create `crates/cli/src/report.rs` containing only:

```rust
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
```

- [ ] **Step 2: Register the module and watch it fail**

Replace `crates/cli/src/main.rs` with:

```rust
mod report;

fn main() {}
```

Run: `cargo test -p chess-analyzer-cli`
Expected: compile errors such as `cannot find function 'render' in this scope` (the helper `mv` in the tests also relies on items from the implementation).

- [ ] **Step 3: Implement the report**

Put this at the top of `crates/cli/src/report.rs`, above the `#[cfg(test)]` line:

```rust
//! Plain-text rendering of a `Review`.

use chess_analyzer_core::classify::MoveClass;
use chess_analyzer_core::eval::Side;
use chess_analyzer_core::review::{MoveReview, Review};

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
```

- [ ] **Step 4: Run the report tests**

Run: `cargo test -p chess-analyzer-cli`
Expected: `3 passed; 0 failed` (dead-code warnings about the unused `render` are fine until the next step).

- [ ] **Step 5: Write the command**

Replace `crates/cli/src/main.rs` with:

```rust
mod report;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};
use chess_analyzer_core::cache::{CachedAnalyzer, open_database};
use chess_analyzer_core::engine::{Analyzer, EngineConfig, Limits, UciEngine, locate_stockfish};
use chess_analyzer_core::game::parse_pgn;
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::review::{ReviewOptions, review_game};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "chess-analyzer",
    about = "Local chess game review powered by Stockfish"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Review a game from a PGN file (use "-" to read stdin).
    Review(ReviewArgs),
}

#[derive(clap::Args)]
struct ReviewArgs {
    /// PGN file, or "-" for stdin.
    pgn: String,
    /// Which game to review when the PGN holds several (1-based).
    #[arg(long, default_value_t = 1)]
    game: usize,
    #[arg(long, default_value_t = 20)]
    depth: u32,
    #[arg(long, default_value_t = 3)]
    multipv: u32,
    #[arg(long, default_value_t = 1)]
    threads: u32,
    /// Stockfish hash size in MB.
    #[arg(long, default_value_t = 256)]
    hash: u32,
    /// Path to the Stockfish executable (default: STOCKFISH_PATH, then engines/stockfish).
    #[arg(long)]
    engine: Option<PathBuf>,
    /// Analysis cache database.
    #[arg(long, default_value = "chess-analyzer-cache.db")]
    cache: PathBuf,
    #[arg(long)]
    no_cache: bool,
    /// Print the full review as JSON instead of text.
    #[arg(long)]
    json: bool,
}

fn read_input(source: &str) -> Result<String> {
    if source == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("reading stdin")?;
        Ok(text)
    } else {
        std::fs::read_to_string(source).with_context(|| format!("reading {source}"))
    }
}

fn review(args: ReviewArgs) -> Result<()> {
    let games = parse_pgn(&read_input(&args.pgn)?)?;
    if args.game == 0 || args.game > games.len() {
        bail!(
            "--game {} is out of range: the PGN contains {} game(s)",
            args.game,
            games.len()
        );
    }
    if games.len() > 1 {
        eprintln!(
            "The PGN contains {} games; reviewing game {} (use --game to choose).",
            games.len(),
            args.game
        );
    }
    let game = &games[args.game - 1];

    let Some(path) = locate_stockfish(args.engine.as_deref()) else {
        bail!(
            "Stockfish was not found. Run scripts/setup-stockfish, set STOCKFISH_PATH, or pass --engine."
        );
    };
    let mut config = EngineConfig::new(path);
    config.threads = args.threads;
    config.hash_mb = args.hash;
    let engine: Box<dyn Analyzer> = Box::new(UciEngine::start(config)?);

    let mut analyzer: Box<dyn Analyzer> = if args.no_cache {
        engine
    } else {
        match open_database(&args.cache) {
            Ok(conn) => Box::new(CachedAnalyzer::new(engine, conn)),
            Err(e) => {
                eprintln!("warning: {e}; continuing without a cache");
                engine
            }
        }
    };

    let options = ReviewOptions {
        limits: Limits {
            depth: args.depth,
            multipv: args.multipv,
        },
        ..ReviewOptions::default()
    };
    let cancel = AtomicBool::new(false);
    let result = review_game(
        game,
        analyzer.as_mut(),
        &options,
        OpeningBook::bundled(),
        &cancel,
        |p| {
            eprint!("\rAnalysing position {}/{}", p.done, p.total);
            let _ = std::io::stderr().flush();
        },
    );
    eprintln!();
    let review = result?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&review)?);
    } else {
        print!("{}", report::render(&review));
    }
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Review(args) => review(args),
    }
}
```

- [ ] **Step 6: Smoke-test against a real game**

Needs Stockfish from Task 5. Create a scratch PGN and review it:

```bash
printf '[White "A"]\n[Black "B"]\n[Result "0-1"]\n\n1. f3 e5 2. g4 Qh4# 0-1\n' > fool.pgn
cargo run -q -p chess-analyzer-cli -- review fool.pgn --depth 10 --no-cache 2>/dev/null
```

Expected output (the engine's suggested move in the last line may differ):

```
A vs B  (0-1)
Opening: A00 Barnes Opening: Fool's Mate
Engine: Stockfish 19 (depth 10, 3 lines)
Accuracy: White 41.2 | Black 100.0   (exact numbers vary a little)

Moves
  1. f3          Book           -0.xx
  1... e5        Book           -0.xx
  2. g4          Blunder          -M1
  2... Qh4#      Book             -M1

Critical moments
  2. g4          Blunder     -0.xx -> -M1, best was Nc3 (lost 4x.x% win chance)
```

Then check the error paths and clean up:

```bash
cargo run -q -p chess-analyzer-cli -- review fool.pgn --game 5 2>&1 | tail -1
cargo run -q -p chess-analyzer-cli -- review fool.pgn --engine /nope --no-cache 2>&1 | tail -1
rm fool.pgn
```

Expected: `Error: --game 5 is out of range: the PGN contains 1 game(s)`, then `Error: Stockfish was not found. Run scripts/setup-stockfish, set STOCKFISH_PATH, or pass --engine.`

- [ ] **Step 7: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/cli
git commit -m "Add the chess-analyzer review command"
```

---

### Task 11: Integration tests against a real Stockfish

**Files:**
- Create: `crates/core/tests/stockfish.rs`

**Interfaces:**
- Consumes: `UciEngine`, `EngineConfig`, `locate_stockfish`, `review_game`, `CachedAnalyzer`, `OpeningBook` from Tasks 4 to 9.

These tests exercise the `UciEngine` written in Task 4, so there is no failing step first: if one fails, fix `engine.rs`. Without a Stockfish they print `SKIPPED` and pass, so contributors without the binary can still run `cargo test`. They assert coarse outcomes (a mate is a mate, a known blunder is a blunder), never exact centipawn values, so they hold across Stockfish versions.

- [ ] **Step 1: Write the tests**

`crates/core/tests/stockfish.rs`:

```rust
//! Integration tests against a real Stockfish. They skip (and say so) if no binary is found:
//! run `scripts/setup-stockfish` or set STOCKFISH_PATH.

use std::sync::atomic::AtomicBool;

use chess_analyzer_core::cache::CachedAnalyzer;
use chess_analyzer_core::classify::MoveClass;
use chess_analyzer_core::engine::{Analyzer, EngineConfig, Limits, UciEngine, locate_stockfish};
use chess_analyzer_core::eval::Eval;
use chess_analyzer_core::game::parse_pgn;
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::review::{ReviewOptions, review_game};

fn engine() -> Option<UciEngine> {
    let Some(path) = locate_stockfish(None) else {
        eprintln!(
            "SKIPPED: Stockfish not found (run scripts/setup-stockfish or set STOCKFISH_PATH)"
        );
        return None;
    };
    Some(UciEngine::start(EngineConfig::new(path)).expect("Stockfish starts"))
}

const SHALLOW: Limits = Limits {
    depth: 8,
    multipv: 3,
};

#[test]
fn reports_its_name() {
    let Some(engine) = engine() else { return };
    assert!(
        engine.engine_id().contains("Stockfish"),
        "{}",
        engine.engine_id()
    );
}

#[test]
fn start_position_gives_ranked_lines_and_a_roughly_equal_score() {
    let Some(mut engine) = engine() else { return };
    let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let analysis = engine.analyze(fen, &SHALLOW).unwrap();
    assert_eq!(analysis.lines.len(), 3);
    assert_eq!(
        analysis.lines.iter().map(|l| l.rank).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(analysis.lines.iter().all(|l| !l.pv.is_empty()));
    match analysis.lines[0].eval {
        Eval::Cp(cp) => assert!(
            cp.abs() < 100,
            "start position should be near equal, got {cp}"
        ),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn white_mate_in_one_is_reported_as_positive_mate() {
    let Some(mut engine) = engine() else { return };
    let analysis = engine
        .analyze("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", &SHALLOW)
        .unwrap();
    assert_eq!(analysis.lines[0].eval, Eval::Mate(1));
    assert_eq!(analysis.lines[0].pv[0], "a1a8");
}

#[test]
fn black_mate_in_one_is_reported_as_negative_mate() {
    let Some(mut engine) = engine() else { return };
    let analysis = engine
        .analyze("r5k1/8/8/8/8/8/5PPP/6K1 b - - 0 1", &SHALLOW)
        .unwrap();
    assert_eq!(analysis.lines[0].eval, Eval::Mate(-1));
    assert_eq!(analysis.lines[0].pv[0], "a8a1");
}

#[test]
fn engine_survives_many_positions_in_a_row() {
    let Some(mut engine) = engine() else { return };
    let game = parse_pgn("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 *")
        .unwrap()
        .remove(0);
    for fen in &game.positions {
        engine.analyze(fen, &SHALLOW).unwrap();
    }
}

#[test]
fn a_full_review_flags_the_blunder_in_the_fools_mate() {
    let Some(mut engine) = engine() else { return };
    let game = parse_pgn("1. f3 e5 2. g4 Qh4# 0-1").unwrap().remove(0);
    let review = review_game(
        &game,
        &mut engine,
        &ReviewOptions {
            limits: SHALLOW,
            ..ReviewOptions::default()
        },
        &OpeningBook::empty(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(review.moves[2].san, "g4");
    assert_eq!(review.moves[2].class, MoveClass::Blunder);
    assert_eq!(review.moves[3].class, MoveClass::Best);
    assert_eq!(
        review.evals[4],
        Eval::Checkmate(chess_analyzer_core::eval::Side::Black)
    );
}

#[test]
fn a_cached_second_review_never_touches_the_engine() {
    let Some(engine) = engine() else { return };
    let game = parse_pgn("1. e4 e5 2. Nf3 Nc6 *").unwrap().remove(0);
    let options = ReviewOptions {
        limits: SHALLOW,
        ..ReviewOptions::default()
    };
    let mut cached = CachedAnalyzer::in_memory(engine).unwrap();
    let first = review_game(
        &game,
        &mut cached,
        &options,
        &OpeningBook::empty(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(cached.misses, 5);
    let second = review_game(
        &game,
        &mut cached,
        &options,
        &OpeningBook::empty(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert_eq!(cached.hits, 5);
    assert_eq!(first, second);
}

#[test]
fn a_position_with_one_legal_move_returns_one_line_despite_multipv() {
    let Some(mut engine) = engine() else { return };
    // Black's king on a8 is in check from the rook; Kb8 is the only legal move.
    let analysis = engine
        .analyze("k7/8/1K6/8/8/8/8/R7 b - - 0 1", &SHALLOW)
        .unwrap();
    assert_eq!(analysis.lines.len(), 1);
    assert_eq!(analysis.lines[0].pv[0], "a8b8");
}

#[test]
fn a_blunder_inside_a_named_opening_line_is_still_flagged() {
    let Some(mut engine) = engine() else { return };
    // "Fool's Mate" is a named line in the bundled opening data.
    let game = parse_pgn("1. f3 e5 2. g4 Qh4# 0-1").unwrap().remove(0);
    let review = review_game(
        &game,
        &mut engine,
        &ReviewOptions {
            limits: SHALLOW,
            ..ReviewOptions::default()
        },
        OpeningBook::bundled(),
        &AtomicBool::new(false),
        |_| {},
    )
    .unwrap();
    assert!(
        review
            .opening
            .as_ref()
            .unwrap()
            .name
            .contains("Fool's Mate")
    );
    assert_eq!(review.moves[2].class, MoveClass::Blunder);
    assert!(review.critical_plies.contains(&3));
}

#[test]
fn a_timeout_restarts_the_engine_and_surfaces_an_error() {
    use chess_analyzer_core::engine::EngineError;
    use std::time::Duration;

    let Some(path) = locate_stockfish(None) else {
        eprintln!("SKIPPED: Stockfish not found");
        return;
    };
    let mut config = EngineConfig::new(path);
    config.timeout = Duration::from_millis(500);
    let mut engine = UciEngine::start(config).expect("Stockfish starts");
    let start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    let err = engine
        .analyze(
            start,
            &Limits {
                depth: 60,
                multipv: 3,
            },
        )
        .unwrap_err();
    assert!(matches!(err, EngineError::Timeout(_)), "{err:?}");

    // The hung process was replaced, so the engine still answers quick requests.
    let quick = engine
        .analyze(
            start,
            &Limits {
                depth: 4,
                multipv: 1,
            },
        )
        .unwrap();
    assert_eq!(quick.lines.len(), 1);
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -p chess-analyzer-core --test stockfish`
Expected: `10 passed; 0 failed` in roughly 5 to 10 seconds. If the output shows `SKIPPED: Stockfish not found`, run Task 5 first.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add crates/core/tests
git commit -m "Add integration tests against a real Stockfish"
```

---

### Task 12: Golden review snapshots

**Files:**
- Create: `data/fixtures/fools_mate.pgn`, `data/fixtures/opera_game.pgn`, `crates/core/tests/golden.rs`, then generated `data/fixtures/*.analysis.json` and `data/fixtures/*.golden.txt`

**Interfaces:**
- Consumes: `review_game`, `ScriptedAnalyzer`, `UciEngine`, `OpeningBook::bundled`.

The golden test replays recorded engine analyses through the review, so it needs no Stockfish and does not move when Stockfish is upgraded. It shows exactly what changes when classification rules or thresholds are tuned. Floats in the snapshot are rounded to one decimal.

- [ ] **Step 1: Create the fixture games**

`data/fixtures/fools_mate.pgn`:

```
[Event "Fool's mate"]
[White "A"]
[Black "B"]
[Result "0-1"]

1. f3 e5 2. g4 Qh4# 0-1
```

`data/fixtures/opera_game.pgn` (Morphy vs the Duke of Brunswick and Count Isouard, Paris 1858):

```
[Event "Casual game"]
[Site "Paris FRA"]
[Date "1858.??.??"]
[White "Paul Morphy"]
[Black "Duke of Brunswick and Count Isouard"]
[Result "1-0"]

1. e4 e5 2. Nf3 d6 3. d4 Bg4 4. dxe5 Bxf3 5. Qxf3 dxe5 6. Bc4 Nf6 7. Qb3 Qe7
8. Nc3 c6 9. Bg5 b5 10. Nxb5 cxb5 11. Bxb5+ Nbd7 12. O-O-O Rd8
13. Rxd7 Rxd7 14. Rd1 Qe6 15. Bxd7+ Nxd7 16. Qb8+ Nxb8 17. Rd8# 1-0
```

- [ ] **Step 2: Write the golden test and the recorder**

`crates/core/tests/golden.rs`:

```rust
//! Golden review tests.
//!
//! Each fixture game in `data/fixtures/` has recorded engine analyses (`<name>.analysis.json`)
//! and an expected review summary (`<name>.golden.txt`). The review is replayed from the
//! recorded analyses, so these tests need no Stockfish and do not depend on its version; they
//! show exactly what changes when classification heuristics or thresholds are tuned.
//!
//! To accept an intended change:  UPDATE_GOLDEN=1 cargo test -p chess-analyzer-core --test golden
//! To re-record analyses:         cargo test -p chess-analyzer-core --test golden -- --ignored record

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use chess_analyzer_core::engine::{
    Analyzer, EngineConfig, EngineError, Limits, PositionAnalysis, ScriptedAnalyzer, UciEngine,
    locate_stockfish,
};
use chess_analyzer_core::game::parse_pgn;
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::review::{Review, ReviewOptions, review_game};

const FIXTURES: &[&str] = &["fools_mate", "opera_game"];

fn fixture(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/fixtures")
        .join(file)
}

/// A stable, human-readable summary. Floats are rounded so tiny platform differences in
/// `exp()` cannot change it.
fn snapshot(review: &Review) -> String {
    let header = |key: &str| review.headers.get(key).cloned().unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!(
        "{} vs {} ({})\n",
        header("White"),
        header("Black"),
        header("Result")
    ));
    match &review.opening {
        Some(o) => out.push_str(&format!("opening: {} {}\n", o.eco, o.name)),
        None => out.push_str("opening: none\n"),
    }
    let acc = |v: Option<f64>| v.map_or("n/a".to_string(), |v| format!("{v:.1}"));
    out.push_str(&format!(
        "accuracy: white={} black={}\n",
        acc(review.accuracy.white),
        acc(review.accuracy.black)
    ));
    out.push_str(&format!("critical plies: {:?}\n", review.critical_plies));
    for m in &review.moves {
        out.push_str(&format!(
            "{:>3} {:<8} {:<10} loss={:>5.1} best={}\n",
            m.ply,
            m.san,
            format!("{:?}", m.class),
            m.loss,
            m.best_san.as_deref().unwrap_or("-")
        ));
    }
    out
}

fn replay(name: &str) -> Review {
    let pgn = std::fs::read_to_string(fixture(&format!("{name}.pgn"))).expect("fixture PGN");
    let recorded = std::fs::read_to_string(fixture(&format!("{name}.analysis.json")))
        .expect("recorded analyses; see the module docs to record them");
    let analyses: Vec<PositionAnalysis> = serde_json::from_str(&recorded).expect("valid analyses");
    let game = parse_pgn(&pgn).expect("valid PGN").remove(0);
    let mut analyzer = ScriptedAnalyzer::new(analyses);
    review_game(
        &game,
        &mut analyzer,
        &ReviewOptions::default(),
        OpeningBook::bundled(),
        &AtomicBool::new(false),
        |_| {},
    )
    .expect("review replays")
}

#[test]
fn reviews_match_their_golden_snapshots() {
    for name in FIXTURES {
        let actual = snapshot(&replay(name));
        let golden_path = fixture(&format!("{name}.golden.txt"));
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(&golden_path, &actual).expect("write golden");
            continue;
        }
        let expected = std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|_| panic!("missing {golden_path:?}; run with UPDATE_GOLDEN=1"));
        assert_eq!(
            actual.replace("\r\n", "\n"),
            expected.replace("\r\n", "\n"),
            "review of {name} changed; if intended, re-run with UPDATE_GOLDEN=1"
        );
    }
}

/// Wraps an analyzer and remembers every answer, in call order.
struct Recording<A: Analyzer> {
    inner: A,
    recorded: Vec<PositionAnalysis>,
}

impl<A: Analyzer> Analyzer for Recording<A> {
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        let analysis = self.inner.analyze(fen, limits)?;
        self.recorded.push(analysis.clone());
        Ok(analysis)
    }

    fn engine_id(&self) -> String {
        self.inner.engine_id()
    }
}

#[test]
#[ignore = "needs Stockfish; re-records data/fixtures/*.analysis.json"]
fn record() {
    let path = locate_stockfish(None).expect("Stockfish (run scripts/setup-stockfish)");
    let engine = UciEngine::start(EngineConfig::new(path)).expect("engine starts");
    let mut recording = Recording {
        inner: engine,
        recorded: Vec::new(),
    };
    for name in FIXTURES {
        recording.recorded.clear();
        let pgn = std::fs::read_to_string(fixture(&format!("{name}.pgn"))).expect("fixture PGN");
        let game = parse_pgn(&pgn).expect("valid PGN").remove(0);
        let options = ReviewOptions {
            limits: Limits {
                depth: 14,
                multipv: 3,
            },
            ..ReviewOptions::default()
        };
        review_game(
            &game,
            &mut recording,
            &options,
            OpeningBook::bundled(),
            &AtomicBool::new(false),
            |_| {},
        )
        .expect("review");
        let json = serde_json::to_string_pretty(&recording.recorded).expect("serialize");
        std::fs::write(fixture(&format!("{name}.analysis.json")), json).expect("write analyses");
    }
}
```

- [ ] **Step 3: Record the analyses (needs Stockfish)**

Run: `cargo test -p chess-analyzer-core --test golden -- --ignored record`
Expected: `1 passed` after about 10 to 15 seconds, and `data/fixtures/fools_mate.analysis.json` and `data/fixtures/opera_game.analysis.json` now exist (roughly 5 KB and 40 KB). Analysis is depth 14, MultiPV 3, one thread, which is deterministic for a given Stockfish build.

- [ ] **Step 4: Generate the snapshots and read them**

Run: `UPDATE_GOLDEN=1 cargo test -p chess-analyzer-core --test golden`
Expected: `1 passed; 0 failed; 1 ignored`, and two `*.golden.txt` files appear.

Now read them; they are the review of two real games and the point of this task is that a person looks at them:

```bash
cat data/fixtures/fools_mate.golden.txt
cat data/fixtures/opera_game.golden.txt
```

Expected for the Fool's Mate (numbers approximate): the opening line `opening: A00 Barnes Opening: Fool's Mate`, `critical plies: [3]`, and the row for ply 3 reading `g4 ... Blunder`. For the Opera Game: an `opening:` line naming a Philidor Defense variation, and `Rd8#` and `Qb8+` marked `Best`. With the starting thresholds the Opera Game's snapshot also contains several `Mistake` and `Great` marks that a strong player may disagree with (for example Black's `Bxf3` and `b5`); that is the starting point for the calibration work the spec calls for, not a bug in this task. Do not edit the snapshots by hand.

- [ ] **Step 5: Run the normal suite and confirm the snapshots are stable**

Run: `cargo test -p chess-analyzer-core --test golden`
Expected: `1 passed; 0 failed; 1 ignored`.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add data/fixtures crates/core/tests/golden.rs
git commit -m "Add golden review snapshots for two fixture games"
```

---

### Task 13: CI, documentation and a final check

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `README.md`, `AGENTS.md`

- [ ] **Step 1: Add the CI workflow**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [master]
  pull_request:

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: Install Stockfish (Linux)
        if: runner.os == 'Linux'
        run: bash scripts/setup-stockfish.sh
      - name: Install Stockfish (Windows)
        if: runner.os == 'Windows'
        run: powershell -NoProfile -ExecutionPolicy Bypass -File scripts/setup-stockfish.ps1
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --workspace
```

The workflow installs Stockfish with the setup scripts, then runs format check, clippy and the whole test suite on Linux and Windows. It is not exercised until the branch is pushed; if the first run fails, fix the workflow or the bash script and commit that.

- [ ] **Step 2: Replace the README**

Replace the one-line `README.md` with:

````markdown
# chess-analyzer

A local game-review tool in the spirit of Chess.com's Game Review. Stockfish evaluates every
position of a game; the app classifies each move (book, brilliant, great, best, good, inaccuracy,
mistake, miss, blunder), scores both players' accuracy and names the opening. Everything runs on
your machine, with no account and no network.

**Status:** the engine core and a command-line reviewer exist. The desktop app (Tauri + React) and
LLM commentary come next. See [AGENTS.md](AGENTS.md) for the architecture and
[docs/superpowers/specs](docs/superpowers/specs) for the design.

## Quick start

1. Install [Rust](https://rustup.rs) (stable, 1.88 or newer).
2. Get Stockfish into `engines/`:
   - Windows: `powershell -ExecutionPolicy Bypass -File scripts/setup-stockfish.ps1`
   - Linux / macOS: `bash scripts/setup-stockfish.sh`

   Or point at your own build with the `STOCKFISH_PATH` environment variable or `--engine <path>`.
3. Review a game:

   ```
   cargo run -p chess-analyzer-cli -- review path/to/game.pgn
   ```

   Useful options: `--depth 20`, `--multipv 3`, `--threads 4`, `--hash 1024`, `--game 2` (for PGN
   files holding several games), `--json`, `--no-cache`.

## Tests

```
cargo test
```

Integration tests that need Stockfish print `SKIPPED` and pass when no binary is found.

## Licenses

This project is MIT licensed. Stockfish (GPLv3) is downloaded separately and run as its own
process. The opening names come from lichess-org/chess-openings (CC0).
````

- [ ] **Step 3: Record the deviations in `AGENTS.md`**

In `AGENTS.md`, under `### Move classification`, replace the paragraph that starts `Starting point (evaluation loss in pawns` and the table that follows it with:

```markdown
Starting point (win-percentage points lost against the engine's best move, from the mover's perspective; defaults of `Thresholds` in `crates/core/src/classify.rs`, which is the single source of truth):

| Win chance lost | Class |
|---|---|
| ≤ 0.5 (or the engine's move) | Best |
| ≤ 5 | Good |
| ≤ 10 | Inaccuracy |
| ≤ 20 | Mistake |
| > 20 | Blunder |

Brilliant, Great, Miss and Book use extra rules on top of these bands (see the doc comments in `classify.rs`). A known opening move stays Book unless it loses more than `book_max_loss` (10 points), so a named line such as the Fool's Mate cannot hide a blunder. Evaluations are `Cp`, `Mate` or `Checkmate` (a finished game), always from White's point of view.
```

Keep the sentence that follows the table (`**These thresholds are a starting point, not final.**` ...).

- [ ] **Step 4: Final verification**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting diff, no clippy output, and `test result: ok` for each target: the CLI (`3 passed`), the core library (`82 passed`), `golden` (`1 passed; 1 ignored`), `stockfish` (`10 passed`), and no failures anywhere.

- [ ] **Step 5: Commit**

```bash
git add .github README.md AGENTS.md
git commit -m "Add CI, a README and the classification notes"
```

---

## Self-review against the spec

- **Structure and build order (step 1: `core` + CLI):** Tasks 1 and 10.
- **`game`:** PGN and move-list construction, multi-game handling (CLI `--game`), illegal moves with the ply: Tasks 3 and 10.
- **`engine`:** UCI driver, threads/hash/depth/MultiPV, timeout → restart → retry once, missing binary: Tasks 4, 5 and 11.
- **`eval`:** two-case evaluation, mate handling, win probability: Task 2 (plus the `Checkmate` variant, needed to score a finished game).
- **`review`:** per-ply pipeline, accuracy, critical moments, opening name, progress and cancel: Task 8.
- **`classify`:** configurable thresholds; brilliant, great, miss and book as separate rules over one input struct: Task 6.
- **`openings`:** ECO lookup from a bundled CC0 dataset, book = plies still in the table: Task 7.
- **`cache`:** SQLite keyed by position, engine and settings; unusable cache degrades to uncached: Tasks 9 and 10.
- **Errors:** Stockfish missing (Tasks 4, 10), hang/crash (Tasks 4, 11), bad PGN (Task 3), cancel (Task 8), cache problems (Tasks 9, 10).
- **Testing:** unit tests (Tasks 2 to 10), real-engine integration tests (Task 11), golden snapshots (Task 12), CI (Task 13). UI tests belong to the app plan.
- **Open items from the spec:** accuracy formula (Task 2, Lichess constants, flagged as not independently re-derived), opening dataset and license (Task 7, CC0 confirmed), brilliant/great/miss rules (Task 6, starting values), Stockfish release (Task 5, `sf_19`).
