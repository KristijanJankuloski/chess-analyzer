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
