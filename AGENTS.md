# AGENTS.md

Project guide for humans and AI coding agents working on **chess-analyzer**: a fully local, desktop-first, Chess.com-style game review application.

## Goal

Recreate a Chess.com-style "Game Review" experience (move classification, evaluation graph, best-move arrows, natural-language explanations) that runs **entirely on the user's machine**: no cloud, no account, no internet dependency.

## Core Principle

> **Stockfish is the authority on chess. The LLM is only a commentator.**

- Stockfish decides *what happened*: evaluations, best moves, principal variations, move classification.
- The local LLM explains *why it matters* in natural language, using only the objective data Stockfish produced.
- The LLM must **never** be asked to calculate chess, judge whether a move is good or bad, or analyze a position on its own. Always pass it structured engine output.

## Architecture

```
                    ┌──────────────────┐
                    │      Tauri 2     │
                    │                  │
                    │  React + TS      │
                    │ react-chessboard │
                    │  chess.js        │
                    └────────┬─────────┘
                             │
                       Tauri IPC (commands/events)
                             │
                    ┌────────▼─────────┐
                    │       Rust       │
                    │                  │
                    │ Review Engine    │
                    │ PGN Parser       │
                    │ Analysis Queue   │
                    └───┬──────────┬───┘
                        │          │
                 ┌──────▼───┐ ┌────▼──────┐
                 │ Stockfish│ │ Local LLM │
                 │ (native, │ │ llama.cpp │
                 │   UCI)   │ │ / Ollama  │
                 └──────────┘ └───────────┘
                        │          │
                        └────┬─────┘
                             ▼
                          SQLite
```

No web server, no PostgreSQL, no cloud services.

## Tech Stack

| Layer | Choice | Notes |
|---|---|---|
| Desktop shell | **Tauri 2** | Small binaries, native process/file access, Windows/Linux/macOS |
| Native backend | **Rust** | Manages Stockfish process, analysis queue, PGN processing, LLM orchestration |
| Frontend | **React + TypeScript** | Highly interactive state: board, arrows, eval, variations, navigation |
| Board UI | **react-chessboard** | MIT licensed. Chessground (the Lichess board) is GPL-3.0-or-later and would make any distributed app GPL |
| Chess rules | **chess.js** | Move generation, FEN, PGN parsing on the UI side |
| Engine | **Native Stockfish** (UCI) | Not WASM: full control over threads, hash, MultiPV, depth, NNUE, Syzygy |
| LLM runtime | **Ollama** first, **llama.cpp** later | Behind a provider abstraction (see below) |
| Storage | **SQLite** + PGN files | Metadata/cache in SQLite; games may live as PGN |

### Alternative considered (not chosen)

.NET + Avalonia UI would work for a native desktop app, but the React ecosystem is a better fit for a visual, highly interactive chess UI (board, move tree, eval graph, animations).

## Analysis Pipeline

```
PGN → Stockfish (every position) → move classification → critical moments → LLM → human explanation
```

1. Parse the PGN into positions (FEN per ply).
2. Run Stockfish on every position (suggested: depth 20-25, MultiPV 3; configurable).
3. Compute evaluation before/after each move and the loss versus the best move.
4. Classify each move (see below).
5. Select **critical moments only**: blunders, mistakes, inaccuracies, great/brilliant moves, key decisions.
6. Send only those positions to the LLM. A 60-move game (~120 positions) should yield roughly 8-15 LLM calls, which keeps a small local model viable.
7. Cache everything (analysis per ply, explanations) in SQLite.

### Live mode

For a game that is still being played (the user types in its moves), the same classifier runs over one persistent Stockfish that keeps searching the newest position until the next move arrives (`crates/core/src/live.rs`). Positions that were never analysed get a quick shallow pass first. A move's class is *provisional* while it is the newest move or rests on a shallow search, and the UI says so.

### Move classification

Starting point (win-percentage points lost against the engine's best move, from the mover's perspective; defaults of `Thresholds` in `crates/core/src/classify.rs`, which is the single source of truth):

| Win chance lost | Class |
|---|---|
| ≤ 0.5 (or the engine's move) | Best |
| ≤ 5 | Good |
| ≤ 10 | Inaccuracy |
| ≤ 20 | Mistake |
| > 20 | Blunder |

Brilliant, Great, Miss and Book use extra rules on top of these bands (see the doc comments in `classify.rs`). A known opening move stays Book unless it loses more than `book_max_loss` (10 points), so a named line such as the Fool's Mate cannot hide a blunder. Evaluations are `Cp`, `Mate` or `Checkmate` (a finished game), always from White's point of view.

**These thresholds are a starting point, not final.** Calibrate them (e.g. consider win-probability-based loss rather than raw centipawns, and account for already-decided positions). Keep thresholds in one configurable place.

## LLM Integration

Do not build around a specific model. Use a provider abstraction:

```
LLMProvider
  ├── Ollama        (initial / development)
  ├── llama.cpp     (embedded, later, for a self-contained product)
  ├── LM Studio     (optional)
  └── OpenAI API    (optional, off by default; the app must work fully offline)
```

The review system calls something like `explainPosition(position, engineAnalysis)` and never depends on which model produced the text.

### Input contract for the LLM

Always provide structured Stockfish output, for example:

```json
{
  "playedMove": "Qd2",
  "bestMove": "Nf3",
  "evaluationBefore": 0.7,
  "evaluationAfter": -0.4,
  "variation": "Nf3 d6 O-O Be7",
  "position": "<FEN>"
}
```

Expected output: a short, plain-language explanation (what went wrong, what the better move achieves), e.g. *"Qd2 is an inaccuracy because it delays development and allows Black to gain time with ...e5. Nf3 develops a piece and prepares castling."*

Guardrails:
- Prompts must state that the engine data is ground truth.
- Do not let the model contradict the engine's classification or best move.
- Keep prompts small and per-move; do not feed the whole game to the model.

## Stockfish Settings (user-configurable)

Expose in the UI / settings: threads, hash size (MB), analysis depth, MultiPV, optional Syzygy tablebase path. NNUE is enabled by default.

## Local Storage

```
~/.mychessreview/            (final app-data location TBD)
    database.db              (SQLite)
    games/                   (PGN files)
    engines/                 (Stockfish binaries)
    models/                  (local LLM weights)
    settings.json
```

Suggested SQLite contents: games (PGN + players/result/metadata), per-ply analysis (ply, eval, best move, PV, depth), cached explanations, settings, annotations. Avoid persisting data that can be reconstructed from PGN + FEN.

## Non-Goals

- No Kubernetes, microservices, Redis, Kafka, vector DB, or ML orchestration frameworks.
- No required account, server, or internet connection.
- No LLM-driven chess evaluation.

## Guidance for Agents

- Keep chess computation (Rust/Stockfish) and language generation (LLM) strictly separated.
- Prefer the simplest thing that works; this is a solo-developer desktop app.
- Keep thresholds, engine settings, and model/provider choice configurable rather than hard-coded.
- Treat items marked "TBD" or "starting point" above as open decisions: confirm with the user before locking them in.

## Open Decisions

- Final classification thresholds / win-probability model.
- Which local model(s) to ship or recommend (Qwen / Gemma / Llama, etc.).
- When to move from Ollama to embedded llama.cpp.
- How Stockfish and model binaries are distributed (bundled vs. downloaded on first run).
- Final app-data directory and app name.
