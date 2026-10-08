# LLM narrator Design

Date: 2026-10-09
Status: Draft for review

## Intent

Chess.com-style review shows a sentence of plain-language commentary for each move. Today that slot is filled by a template (`describeMove` in `app/src/lib/commentary.ts`) built from the classification alone. This milestone fills it with an explanation written by a local LLM: what went wrong, and what the better move achieves.

Success: opening a critical move in a finished review shows a short, accurate explanation that never contradicts the engine, generated on a 3-8B local model. In a live game, pressing Explain on a move sends it to the model. Everything stays local, and the app is fully usable with the LLM off, unreachable or missing.

Core principle (from `AGENTS.md`) is unchanged: **Stockfish is the authority on chess; the LLM is only a commentator.** The model is never asked to calculate, judge a move or read a board. It paraphrases a short list of facts that Rust computed from the engine's own lines.

## Decisions made during brainstorming

| Topic | Decision |
|---|---|
| When explanations are generated | Hybrid. After a review finishes, every critical move is explained in the background and cached. Any other move can be explained on demand. Live games are on demand only: an Explain button on the selected move, and nothing is sent to the model until it is pressed. Tried first: an automatic explanation of the newest move after an idle delay, dropped because a local 7-8B model is slow, and a call started automatically would compete with Stockfish for the CPU for no reason the user asked for. |
| Backends | One OpenAI-compatible HTTP client (`{base_url}/chat/completions`) with a configurable base URL, model and optional API key. It covers Ollama (`http://localhost:11434/v1`), LM Studio, llama.cpp's `llama-server` and OpenAI. The `LlmProvider` trait stays so an embedded llama.cpp can be added later. |
| What the model writes | Per-move explanations only: 2-3 plain sentences. No game summary, no Q&A. |
| Grounding | Rust computes a **facts digest** from the played move and the engine's lines (captures, checks, mates, material along the lines, pieces left hanging, the opponent's best reply). The model only paraphrases it. No separate output validator in this milestone. |
| Model class | Prompts are sized for a 3-8B instruct model on CPU or a modest GPU. No specific model is hard-coded or named in the UI. |
| Structure | Facts in `crates/core` (pure chess). Everything language-related in a new `crates/narrator` that depends on core. Core never depends on narrator. |
| Default | Off. Nothing touches the network until the user enables it in Settings. |
| Cache | One SQLite table keyed by a hash of the exact prompt, model and prompt version. It is independent of any game id, so review and live share it. |
| Streaming | Not in this milestone. The template sentence is shown until the text lands. |

## Structure

```
crates/
  core/
    src/facts.rs          # NEW: ExplainRequest -> Digest (pure, shakmaty, no I/O)
    src/settings.rs       # + LlmSettings
    src/live.rs           # + a query for the current analysis of a ply
  narrator/               # NEW crate; depends on core
    src/provider.rs       # LlmProvider, Completion, LlmError, LlmStatus
    src/openai.rs         # OpenAiCompatible (ureq)
    src/prompt.rs         # PROMPT_VERSION, system prompt, digest rendering
    src/cache.rs          # ExplanationCache (SQLite)
    src/service.rs        # ExplainService: queue, worker thread, events
  cli/                    # + `explain` subcommand
app/
  src-tauri/              # + commands and the explain-event channel
  src/                    # commentary slot, live Explain button, Settings section
```

The dependency direction is the point of the split: `core::facts` is the only code that looks at a board, and `narrator` sees nothing but its output.

## `core::facts`

### Input

```rust
pub struct ExplainRequest {
    pub fen_before: String,       // position before the move
    pub review: MoveReview,       // class, evals, loss, best_san, best_pv (UCI), ...
    pub reply_pv: Vec<String>,    // engine PV for the position after the move (UCI); may be empty
    pub opening: Option<Opening>,
    pub depth: Option<u32>,       // depth the evidence rests on, for live provisional notes
}
```

`MoveReview` already holds everything the review and live pipelines know about a move, so both build the same request. `reply_pv` is the opponent's best reply: in a stored review it is the next move's `best_pv`, in live it comes from the analysis of the position after the move. It is empty for the final move, and the digest then omits the reply facts.

### Output

`Digest` is a fixed list of verifiable facts, rendered by `narrator::prompt` as short tagged lines. It contains:

- who moved and the move class (as given, not re-judged);
- the evaluation before and after in plain terms ("equal", "White is better", "mate in 3 for Black") and the win chance lost;
- what the played move did: capture (with the captured piece), check, castle, promotion, and whether the moved piece or another piece was left attacked and undefended;
- what the best move does, from the same list;
- the opponent's best reply to the played move, in SAN;
- the material balance now, after the reply line and after the best line, so "this loses a knight" is a computed fact;
- the first 4-6 moves of the best line in SAN.

Each fact is optional. A PV move that does not replay drops its own fact; if even the basics cannot be built, no model call is made. The digest is deterministic and unit-testable.

## `narrator` crate

### Provider

```rust
pub trait LlmProvider: Send + Sync {
    fn complete(&self, req: &Completion) -> Result<String, LlmError>;
    fn check(&self) -> LlmStatus;   // reachable? is the configured model listed?
}
pub struct Completion { system: String, user: String, max_tokens: u32, temperature: f32 }
```

Blocking, to match the thread-based `ReviewJobs` and `LiveSession` without adding an async runtime. `OpenAiCompatible { base_url, model, api_key: Option<String> }` posts with `ureq`. `check` calls `GET {base_url}/models`. Defaults: temperature about 0.3, `max_tokens` about 160. Timeouts are generous (seconds to connect, minutes in total) because a 7B model on a CPU is slow, especially on the first call after the server loads it.

`LlmError`: `Unreachable`, `Timeout`, `Http { status, snippet }`, `BadResponse`, `Empty`. A 404 about the model is reported as "model `X` not found on that server".

### Prompt

The system prompt is a versioned constant (`PROMPT_VERSION`). It states that the facts are engine ground truth; forbids naming any piece, square or move that is not in them; forbids re-judging the class or the best move; and fixes the shape (2-3 plain sentences, no markdown, no numbers beyond those given). It carries one or two short worked examples. The user message is the rendered digest. One prompt is one move and a few hundred tokens; the whole game is never sent.

The reply is trimmed and stripped of stray markdown and quotes. An empty reply, or one far over the length cap, is a failed call (`Empty` / `BadResponse`) and is not cached.

### Cache

```
explanations(key TEXT PRIMARY KEY, text, model, prompt_version, created_at)
key = SHA-256(prompt_version || model || system || user)
```

It lives in the same SQLite database as saved games, behind its own connection, like `GameStore` and `CachedAnalyzer`. Changing the model or the prompt misses the cache by construction, so nothing stale is shown. A live move whose analysis deepens produces a different digest and therefore a new key, and an older explanation remains a hit if the user goes back to it. "Regenerate" overwrites the row. If the database is unavailable, explanations work uncached with a warning.

### Service

`ExplainService` runs one worker thread with a queue, making one model call at a time. On-demand requests go to the front of the queue. The service builds each `ExplainRequest` itself, never from UI-supplied engine data:

- `Saved { game_id }`: read the stored game and review; `fen_before` from the game, `reply_pv` from the next move's `best_pv`.
- `Live`: ask the `LiveSession` (new query) for the current analysis of that ply.

A cancel drops the queued items for a scope. A call already in flight cannot be interrupted (it is a blocking HTTP read); it finishes, its text is cached, and the UI ignores it if the key is stale.

## Settings

`Settings` gains a nested block, `#[serde(default)]` so existing settings files keep loading, with ts-rs exports like the rest:

```
llm: { enabled: false,
       base_url: "http://localhost:11434/v1",
       model: "",
       api_key: None,
       auto_explain: true }       // explain critical moves after a review; live is always manual
```

- `validate()` checks that the URL parses and, when enabled, that a model name is set.
- Changing `llm` fields does **not** rebuild the live Stockfish session. The existing rebuild check applies to the engine fields only.
- The API key is stored in plain text in `settings.json`. This is fine for the intended Ollama setup (no key); keychain storage is out of scope. The Settings screen says so next to the field.

## Tauri app and UI

### Commands and events

| Command | Purpose |
|---|---|
| `llm_status()` | Runs `check()`; returns reachable / model present / error. |
| `explanations_lookup(source, plies)` | Cache read only. Never calls the model; works while the LLM is disabled. |
| `explain_move(source, ply, force)` | Returns `{ key, text? }` at once. If not cached, queues a call at the front. `force` regenerates. |
| `explain_critical(source)` | Queues every critical ply of a saved game not already cached. Started automatically when a review finishes (if `auto_explain`) and when a saved game is opened; cached plies cost nothing. |
| `cancel_explain(scope)` | Drops queued items for that scope. |

`source` is `Saved { game_id }` or `Live`. Events go out on the `explain-event` channel: `Ready { key, ply, text }`, `Failed { key, ply, error }`, `Progress { done, total }`. The UI keeps the latest `key` per move, drops events for keys it no longer wants, and looks text up by key (the same stale-event discipline as live revisions). All types derive `ts_rs::TS` into `app/src/generated/`. The commands are added to the single `Api` interface with a Tauri implementation and a scriptable fake.

### Review screen

The commentary line shows the template immediately. LLM text replaces it with a small "AI" badge when it arrives, with a "Regenerate" action. Moves that are not critical show an "Explain" button. While the batch runs, a chip "Explaining 3/9" with a cancel is shown.

### Live screen

The commentary line shows the template for the selected move, with an "Explain" button. Pressing it calls `explain_move(Live, ply)`. Nothing is sent to the model otherwise: there is no timer and no automatic live call.

- The button works on any classified move, not only the newest, using that move's current (frozen or still-deepening) analysis.
- While the call runs the button shows a busy state. Moving on to another move does not cancel it; its text is cached and shown when the user returns to that move. A new move or take-back sends `cancel_explain("live")` to drop anything still queued.
- If the move is still provisional (shallow search, or the game is not over), the text carries "based on depth N, may change". Pressing Explain again after the search has deepened produces a new explanation, because the digest differs.
- The model and Stockfish compete for CPU while the explanation is generated. Because the user chooses when, this is their trade-off.
- Cached explanations for moves in the current game show without pressing anything (`explanations_lookup`), as long as the digest is unchanged.
- The button is hidden or disabled, with the reason, when commentary is disabled or the server is unreachable.

### Settings screen

A "Commentary" section: enable toggle, base URL, model, optional API key, auto-explain toggle (for reviews only), and a "Test connection" button showing the `llm_status` result. A one-line hint says a 7-8B instruct model is recommended, without naming one.

### Disabled or unreachable

Previously cached text still shows. Generation buttons are hidden or disabled with the reason. The template covers everything else. A review never fails because of the LLM.

## Errors

- **Disabled:** no calls are made; cached text is shown.
- **Unreachable, timeout, HTTP error, empty or oversized reply:** a `Failed` event; the UI keeps the template and offers "Couldn't generate: retry". Failures are never cached.
- **Batch:** one `Unreachable` stops the whole batch. Other per-move errors continue; three in a row abort it. A single summary is shown, not one message per move.
- **Digest problems:** the affected fact is left out; if the basics cannot be built, no call is made.
- **Database unavailable:** explanations work uncached, with a warning.
- **App closes mid-call:** nothing is persisted, because a row is written only after success.

## Testing

- **`core::facts`:** unit tests on hand-built positions for captures, a hanging piece, checks, mate, material deltas, promotion and a missing `reply_pv`. Golden digest snapshots for the two fixture games, in the style of the existing golden reviews.
- **`narrator`:**
  - A `ScriptedProvider` fake for service tests: on-demand priority over the batch, skipping cached plies, cancel, the abort rules and stale keys.
  - Cache key sensitivity to model and prompt version.
  - `OpenAiCompatible` against a small stub HTTP server on a local port: success, 404, malformed JSON, timeout and `check()`.
  - Prompt snapshot tests.
  - One integration test against a real local model, skipped when none is reachable (as with the Stockfish tests).
- **Frontend (fake `Api`):** the template-to-AI swap and badge, regenerate, disabled and cached states, and failure. The live Explain button: it sends exactly one request per press, shows busy and then the AI text, shows a provisional note on a shallow analysis, is disabled when commentary is off, and nothing is sent without a press. The Settings section.
- **End-to-end:** `npm run e2e:explain` drives the real app against a stub OpenAI-compatible server, so it needs no model.
- **Tuning tool:** `chess-analyzer explain game.pgn [--dry-run]` prints each critical move's digest and explanation; `--dry-run` prints only the prompts. Prompt wording and the digest fields are tuned with it against a real model, as thresholds were tuned with the review CLI.
- The generated TypeScript drift check in CI covers the new types.

## Amends the live game spec

`2026-10-08-live-game-design.md` says "This feature has no LLM". This milestone adds one narrow exception: an Explain button on the live screen that sends the selected move to the model, using the shared cache. Nothing is sent automatically. Live analyses are still not stored in SQLite; only explanation text, keyed by prompt hash, is.

## Out of scope

- A game summary paragraph and a Q&A chat.
- Streaming responses.
- A content validator that rejects explanations mentioning things outside the digest.
- An embedded llama.cpp and any model download or pull management.
- Keychain storage for the API key.
- Automatic explanation of live moves (an idle-delay trigger was tried in the design and dropped; it can be revisited if local models get fast enough).
- Variations, as everywhere in v1.

## Left for the implementation plan

- The exact digest fields and the rule for a "hanging piece".
- Prompt wording and worked examples, tuned with `chess-analyzer explain` against a real model.
- The `ureq` version and its TLS feature (needed for `https://` providers such as OpenAI).
- How `LiveSession` exposes the current analysis of a ply to the service.
- The default values for `max_tokens`, temperature, the length cap and the timeouts.
- The wording of the recommended-model hint.
