# chess-analyzer

A local game-review tool in the spirit of Chess.com's Game Review. Stockfish evaluates every
position of a game; the app classifies each move (book, brilliant, great, best, good, inaccuracy,
mistake, miss, blunder), scores both players' accuracy and names the opening. Everything runs on
your machine, with no account and no network.

**Status:** the engine core, a command-line reviewer and a desktop app for reviewing, recording and following live games exist.
LLM commentary comes next. See [AGENTS.md](AGENTS.md) for the architecture and
[docs/superpowers/specs](docs/superpowers/specs) for the design.

## Getting started

You need [Rust](https://rustup.rs) (stable, 1.88 or newer) and a Stockfish binary. Run everything
from the repository root.

### 1. Get Stockfish

The setup script downloads Stockfish 19 into `engines/` (about 100 MB, ignored by git):

| Platform | Command |
|---|---|
| Windows (PowerShell) | `powershell -ExecutionPolicy Bypass -File scripts/setup-stockfish.ps1` |
| Linux / macOS | `bash scripts/setup-stockfish.sh` |

It finishes with `Installed id name Stockfish 19 at ...`. The Linux/macOS script has not been run
on those platforms yet; if it fails, download Stockfish yourself and use the next option.

Already have Stockfish? Skip the script and either set the `STOCKFISH_PATH` environment variable
or pass `--engine <path>` on every run.

### 2. Review a game

A sample game is included, so you can try it straight away:

```
cargo run -p chess-analyzer-cli -- review data/fixtures/opera_game.pgn --depth 12
```

The first run compiles the project, which takes a few minutes. After that, this review takes a few
seconds. You should see something like:

```
Paul Morphy vs Duke of Brunswick and Count Isouard  (1-0)
Opening: C41 Philidor Defense
Engine: Stockfish 19 (depth 12, 3 lines)
Accuracy: White 99.0 | Black 85.4

Moves
  1. e4          Book           +0.30
  1... e5        Book           +0.41
  ...

Critical moments
  4... Bxf3      Inaccuracy  +0.81 -> +1.89, best was Nd7 (lost 9.3% win chance)
  6... Nf6       Mistake     +1.24 -> +2.52, best was Qf6 (lost 10.4% win chance)
  7. Qb3         Great       +2.52 -> +2.52 (lost 0.0% win chance)
  ...
```

Evaluations are from White's point of view (`+` favours White). `M3` / `-M3` means a forced mate in
3 for White / Black, and `1-0 #` / `0-1 #` marks the checkmate itself.

To review your own game, export it as a PGN file (both Chess.com and Lichess offer a PGN download)
and point the command at it:

```
cargo run -p chess-analyzer-cli -- review path/to/your_game.pgn
```

Use `-` instead of a path to read from standard input.

### Options

| Option | Default | Meaning |
|---|---|---|
| `--depth N` | 20 | Search depth per position. Higher is stronger and slower; `12` is a quick look. |
| `--multipv N` | 3 | Lines Stockfish reports per position. Needed for "Great" moves; use at least 2. |
| `--threads N` | 1 | CPU threads for Stockfish. |
| `--hash MB` | 256 | Stockfish hash memory in MB. |
| `--game N` | 1 | Which game to review when the PGN file contains several. |
| `--engine PATH` | auto | Stockfish executable. Otherwise `STOCKFISH_PATH`, then `engines/stockfish`. |
| `--json` | off | Print the full review as JSON instead of the text report. |
| `--cache FILE` / `--no-cache` | `chess-analyzer-cache.db` | Analyses are cached in a SQLite file, so re-reviewing a game is nearly instant. |

Depth must be 1 to 60, MultiPV 1 to 10, threads 1 to 256 and hash 1 to 65536 MB; anything else is refused.

### Good to know

- Only standard chess is supported. PGNs with a `[Variant ...]` tag such as Chess960 or Antichess
  are rejected with an error.
- Variations and comments in a PGN are ignored; only the main line is reviewed.
- The cache file is created in the directory you run the command from.
- Accuracy is the plain average of each move's accuracy, using Lichess's published per-move
  formula. The numbers will not match Chess.com's or Lichess's own exactly.
- Move classes use starting thresholds that still need calibrating on real games (see
  [AGENTS.md](AGENTS.md)), so treat a "Mistake" as a hint rather than a verdict.

### Troubleshooting

| Message | Fix |
|---|---|
| `Stockfish was not found` | Run the setup script from the repository root, or set `STOCKFISH_PATH` / pass `--engine`. |
| `no game found in the PGN` | The file has no moves. Check that it is a PGN export of a game. |
| `illegal or unreadable move "..." at ply N` | The PGN has a move that is not legal at that point (or a typo). `N` counts half-moves from the start. |
| `unsupported variant` | The game is not standard chess. |
| `the engine did not answer within ...` | The position took longer than 120 s at that depth. Lower `--depth`. |

## Desktop app

The desktop app (Tauri + React) shows a review the way Chess.com does: the board with a class badge on each move and an arrow for the move the engine preferred, an evaluation bar and graph, the classified move list, both players' accuracy and the opening. Reviews stream in while Stockfish works, and every finished review is saved so you can reopen it from the Games screen.

You need [Node.js](https://nodejs.org) 20 or newer as well as Rust and Stockfish (see above).

```
cd app
npm install
npm run tauri dev
```

The first run compiles Tauri, which takes several minutes. Then:

- **Games:** paste a PGN or open a PGN file and press Review. Recent games are listed; click one to reopen it.
- **Record:** play a game over the board here, by clicking a piece and then its square or by dragging. The board enforces the rules (castling, en passant, promotion with a piece chooser) and ends the game at checkmate, stalemate or insufficient material. Repetitions and the fifty-move rule are left to you, because only a player claiming the draw ends a real game: pick the result yourself. Add the players' names, take moves back, and press "Review this game" to analyse it like any PGN.
- **Live:** follow a game that is being played somewhere else, a tournament broadcast say. Enter its moves for both sides, on the board or by typing them (`e4`, `nf3`, `O-O`), and the evaluation bar, the engine's best lines and a class badge on every move update while Stockfish thinks. A green arrow is the engine's best move now; a red arrow is the move that should have been played instead of a mistake or blunder. A badge with a dashed outline is provisional: the engine has not looked deeply enough yet, so it can still change. Joining a game that is already under way works too: type the moves so far, and the earlier moves get a quick look while the newest position gets the deep one. Leaving the tab pauses the search, and the game is remembered when you close the app. "Review this game" hands it to the full review.
- **Settings:** the Stockfish path (empty means find it automatically), depth, lines, threads and hash. "Check engine" starts Stockfish to prove it works.
- **Keyboard:** left and right arrows step through the moves, Home and End jump to the start and end.

Games, settings and the analysis cache live in `%APPDATA%\com.chessanalyzer.app` on Windows.

To see the interface in an ordinary browser without the Rust side, run `npm run dev` in `app` and open `http://localhost:1420/?demo`. Run the frontend tests with `npm test`. `npm run e2e:review`, `npm run e2e:record` and `npm run e2e:live` check the real app end to end (see `app/e2e`; start the app with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` first).

## Tests

```
cargo test
```

Integration tests that need Stockfish print `SKIPPED` and pass when no binary is found.

## Licenses

This project is MIT licensed. Stockfish (GPLv3) is downloaded separately and run as its own
process. The opening names come from lichess-org/chess-openings (CC0).
