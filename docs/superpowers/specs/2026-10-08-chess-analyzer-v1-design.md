# chess-analyzer v1 Design

Date: 2026-10-08
Status: Draft for review

## Intent

A fully local, desktop Chess.com-style game review tool. It is primarily a personal tool, published as open source (MIT) so others can clone and build it. Success for v1: load a PGN or record a game by hand, run a full review with Stockfish, and see the classified moves, accuracy, eval graph, best-move arrows and opening name, with no network access.

Core principle (from `AGENTS.md`): Stockfish is the authority on chess; any LLM only explains engine output. The LLM is **not** part of v1.

## Decisions made during brainstorming

| Topic | Decision |
|---|---|
| Audience | Personal first; open source. No installers or cross-platform polish in v1. Development and testing on Windows; code kept portable. |
| v1 scope | Stockfish-only. LLM commentary is a second milestone. |
| Game input | Paste/open a PGN, or record a game by hand on the board. No Chess.com/Lichess import. |
| Variations | Not in v1. A game is a single linear sequence of moves. The data model must not block adding variations later. |
| Review features | Everything: basic classification, brilliant/great/miss/book, per-player accuracy, eval graph, best-move arrows, opening names. |
| Mate scores | First-class. Evaluation is `Cp(n)` or `Mate(n)`. |
| Architecture | Rust core library with thin UI (Approach 1). |
| Stockfish | Fetched by a setup script into `engines/` (not committed) and found next to the app or in `engines/`; an optional setting overrides the path. Bundling it as a Tauri sidecar is deferred until installers are in scope. |
| Board library | `react-chessboard` (MIT). Chessground is GPL-3.0 and would make the distributed app GPL. |
| LLM | Out of v1. Delivery mechanism (Ollama vs. downloaded GGUF) decided at that milestone. |

## Structure

```
chess-analyzer/
  crates/
    core/        # game model, PGN, UCI driver, review pipeline, SQLite cache
    cli/         # `chess-analyzer review game.pgn`, thin wrapper over core
  app/
    src-tauri/   # Tauri shell: commands and events over core
    src/         # React + TS: react-chessboard board, eval bar/graph, move list
  data/          # opening table, fixture PGNs
  scripts/       # fetches the Stockfish binary for the host OS
```

Build order (each step is its own plan and is usable on its own):
1. `core` + CLI
2. Tauri app (load PGN, live review, review screen)
3. Record-by-hand mode

## `core` crate

- **`game`**: PGN text or a move list becomes a `Game` (headers plus one position per ply: FEN, SAN, UCI move), using `shakmaty`. Recorded games build the same type. Multi-game PGNs: the user picks one game in v1.
- **`engine`**: UCI driver around a Stockfish child process. `analyze(fen, limits) -> PositionAnalysis` returns the top N lines (MultiPV), each with `Eval`, principal variation and depth. Applies threads, hash, depth/nodes, MultiPV. Defaults: depth 20, MultiPV 3, all configurable.
- **`eval`**: `Eval` is `Cp(i32)` or `Mate(i32)`, stored from White's point of view. `win_probability(eval)` maps `Cp` through a logistic curve and `Mate` to 100% or 0% by sign. Accuracy, classification and the graph all use it.
- **`review`**: pipeline from `Game` + engine to `Review`. Per ply: analyse the position before and after the move, compute the win-probability loss against the best move, classify. Then per-player accuracy, critical moments, and opening name.
- **`classify`**: pure functions from a move's analysis to a class. Thresholds live in one config struct. Brilliant, great, miss and book are separate rules over the same inputs (top lines, loss, material/exchange check for sacrifices). Starting thresholds are in `AGENTS.md` and are to be calibrated against real games using the CLI.
- **`openings`**: ECO lookup from a bundled dataset (candidate: the lichess-org/chess-openings dataset; confirm its license when planning). Book moves are plies still inside the table.
- **`cache`**: SQLite keyed by FEN, engine version and settings, so re-reviews and shared openings don't recompute positions.

Accuracy: per-move accuracy from win-probability loss using the Lichess open formula (to be confirmed and referenced in the plan), averaged per player.

Mate handling: throwing away a forced mate with a non-mating move is a miss or blunder; walking into a forced mate is a blunder; a slower mate (M5 instead of M2) is not penalised like losing the win. The UI shows "M3" / "-M3" and pins the bar at the edge.

Data flow: `PGN -> Game -> engine (per position, cached) -> review -> Review`. Progress (ply N of M) is reported through a callback or channel. `Review` is plain serializable data, so the CLI prints it, the UI renders it and the later LLM milestone can consume it.

## Tauri app and UI

Commands (typed wrapper in TS, types generated from the Rust structs):
- `start_review(source)`: PGN text or recorded moves; returns a review id and runs on a background thread.
- `cancel_review(id)`, `get_review(id)`, `get_settings`, `set_settings`, `list_games`.

Events: `review-progress`, `review-partial` (per-ply classification as it completes, so the graph and move list fill in live), `review-complete`.

Screens:
- **Review**: react-chessboard board with arrows and highlights, eval bar, clickable eval graph, classified move list, per-player accuracy, opening name.
- **New game**: paste PGN, open file, or switch to record mode.
- **Record mode**: same board; chess.js validates each dragged move; take-back; "Review" sends the moves to `start_review`.
- **Settings**: Stockfish path, threads, hash, depth, MultiPV. First-run check for a missing binary.

State: the board position is derived from the selected ply of the current `Review`. The UI performs no evaluation. chess.js is used only to validate moves in record mode.

## Errors

- Stockfish missing or won't start: typed `EngineError`; the app prompts to set up Stockfish and opens Settings; the CLI prints the setup-script instruction.
- Engine crash or hang: per-position timeout, restart and retry once; on a second failure the review stops with a partial result. Analysed positions stay cached, so a retry resumes.
- Bad PGN: `GameError` with the line or ply. Illegal moves are reported, not skipped.
- Cancel: stops the engine promptly and keeps the cache.
- SQLite unavailable or corrupt: review still works uncached, with a warning.

## Testing

- Unit tests: `eval` conversions (including mate cases), each `classify` rule on hand-built inputs, and PGN edge cases (promotion, castling, SAN ambiguity, results). Thresholds are config, so tests don't break when tuning.
- Engine integration tests: real Stockfish at low depth on fixture PGNs, asserting coarse outcomes (a known blunder is a blunder; a forced mate is `Mate`) and not exact centipawns. Skipped if no binary is found.
- Golden review snapshots on a few fixture games, to see what changes when heuristics are tuned.
- UI: component tests for the move list and eval graph, plus a startup smoke test.
- CI: one workflow running `cargo test` and the frontend tests.

## Out of scope for v1

- LLM commentary (second milestone).
- Variations and the variation tree.
- Playing against the engine.
- Chess.com / Lichess import.
- Installers, signing, auto-update, and verified macOS/Linux support.
- Opening-explorer, player profiles, and multi-game databases beyond a simple list of past reviews.

## Open items to settle during planning

- Exact accuracy formula and its reference.
- Opening dataset choice and license check.
- Concrete rules and thresholds for brilliant, great and miss, to be tuned on real games via the CLI.
- Stockfish release version and download source for the setup script.
