# Move commentary Design

Date: 2026-10-09
Status: Draft for review

## Intent

Chess.com-style review shows a sentence of plain-language commentary for each move. Today that slot holds a template built from the classification alone (`describeMove` in `app/src/lib/commentary.ts`): "Nf6 is a mistake. Best was Qf6. It cost 13.8% win chance." It says *that* a move was bad, not *why*.

This work fills the slot with a real explanation, in two milestones that share one foundation:

1. **Milestone 1: Stockfish-only commentary.** Rust computes a typed **facts digest** from the played move and the engine's own lines, and a **template renderer** turns it into 2-3 plain sentences. It is instant, deterministic, fully offline, and works in review and in live games.
2. **Milestone 2: optional local-LLM coach.** A second renderer over the *same digest* hands the facts to a local model for more fluent prose. It sits behind one toggle in Settings, off by default. Built only if milestone 1's output proves too flat in practice.

Success for milestone 1: opening a critical move in a finished review, or selecting a move in a live game, shows a correct sentence that names the actual cause drawn from the engine's lines (for 6...Nf6 in the Opera Game: the king is driven to e7 and loses castling), not just a rating. Nothing leaves the machine, and nothing needs installing beyond Stockfish.

Core principle (from `AGENTS.md`) is unchanged: **Stockfish is the authority on chess; language is only a commentator.** Neither renderer calculates, judges a move or reads a board. Both only express facts that Rust computed from engine output.

## Why the digest comes first

A manual test on the Opera Game (6...Nf6, a 13.8-point mistake) showed that the facts are where the value is. A model given only numbers and a line would have to guess; given a digest it wrote a good explanation. A template over the same digest can say the same thing. The LLM adds prose fluency, and it adds risk: a 3B model (llama3.2) echoed its input and named the wrong side, while an 8B model (llama3.1) was good but slow. The digest is the part both renderers need and the part that decides correctness, so it is built first and on its own merits.

## Decisions made during brainstorming

| Topic | Decision |
|---|---|
| Is an LLM needed? | No. The model only paraphrases the digest, so a template over the digest can carry the same information, instantly and without hallucination. The LLM is kept as an optional second milestone, because fluency, variety and tone are what templates do worst. |
| Order | Milestone 1 (digest + templates) ships first and stands alone. Milestone 2 is built only after the user has read milestone-1 output on real games and judged it too flat, and gets its own plan. |
| Detectors | Written in-house on `shakmaty`. No reusable chess-commentary library exists: the best-known tactic tagger (lichess-puzzler) is AGPL-3.0 and the repo is MIT, so its code is not copied; detectors are written from the public theme definitions. |
| Sentence assembly | Plain Rust (`match` and `format!`), English only, all wording in one module. A template engine or Fluent is deferred until someone wants translations or editable wording. |
| Digest shape | Typed facts (an enum), not strings, so both renderers match on them and neither parses text. Facts are ranked by significance. |
| Where it runs | In Rust (`core`), as a field of the review itself: `review::review_move`, the one function that classifies a move for both a finished review and a live game, also writes its commentary from the two analyses it already holds. No new Tauri command, event or setting. The CLI and the app share it, and chess logic stays out of the UI. The old `describeMove` stays only as the fallback while a move has no commentary. |
| Live games | Milestone 1 shows commentary for the selected move automatically: it costs microseconds and no engine time, so it cannot compete with the live search. Milestone 2 adds an Explain button. |
| Coach toggle (milestone 2) | One switch in Settings (`llm.enabled`), off by default. Off means exactly the milestone-1 app: no connection of any kind, no LLM controls anywhere. |
| LLM backends (milestone 2) | One OpenAI-compatible HTTP client (`{base_url}/chat/completions`) with a configurable base URL, model and optional API key. It covers Ollama (`http://localhost:11434/v1`), LM Studio, llama.cpp's `llama-server` and OpenAI. The `LlmProvider` trait stays so an embedded llama.cpp can be added later. |
| LLM output (milestone 2) | Per-move explanations only, 2-3 plain sentences. No game summary, no Q&A, no streaming. |
| LLM model class (milestone 2) | Prompts are sized for a 7-8B instruct model. No specific model is hard-coded or named in the UI. |
| LLM generation (milestone 2) | Hybrid. After a review finishes, critical moves are explained in the background and cached; any other move on demand. Live is on demand only (an Explain button). An automatic idle-delay trigger for live was tried in the design and dropped: a local model is slow and would compete with Stockfish for the CPU. |
| LLM cache (milestone 2) | One SQLite table keyed by a hash of the exact prompt, model and prompt version. It is independent of any game id, so review and live share it. |

## Structure

```
crates/
  core/
    src/facts.rs          # NEW (M1): CommentaryInput -> Digest (pure, shakmaty, no I/O)
    src/facts/            # NEW (M1): one small file per detector (exchange, king, motifs: fork and pin)
    src/commentary.rs     # NEW (M1): Digest -> text (template renderer)
    src/review.rs         # MoveReview.commentary, written by review_move; digest_for; backfill_commentary
    src/store.rs          # fills in commentary for reviews saved before it existed
    src/settings.rs       # + LlmSettings (M2)
  narrator/               # NEW crate (M2); depends on core
    src/provider.rs       # LlmProvider, Completion, LlmError, LlmStatus
    src/openai.rs         # OpenAiCompatible (ureq)
    src/prompt.rs         # PROMPT_VERSION, system prompt, Digest -> prompt text
    src/cache.rs          # ExplanationCache (SQLite)
    src/service.rs        # ExplainService: queue, worker thread, events
  cli/                    # + `review --commentary` and `--facts` (M1)
app/
  src-tauri/              # unchanged in M1; explain commands and events (M2)
  src/                    # commentary line reads MoveReview.commentary (M1); AI badge, Explain button, Settings section (M2)
```

The dependency direction is the point of the split: `core::facts` is the only code that looks at a board, `core::commentary` and `narrator` see nothing but its output, and `core` never depends on `narrator`.

---

# Milestone 1: Stockfish-only commentary

## `core::facts`

### Input

```rust
pub struct CommentaryInput {
    pub fen_before: String,       // position before the move
    pub review: MoveReview,       // class, evals, loss, best_san, best_pv (UCI), ...
    pub reply_pv: Vec<String>,    // engine PV for the position after the move (UCI); may be empty
    pub opening: Option<Opening>,
}
```

`MoveReview` already holds everything the review and live pipelines know about a move, so both build the same input. `reply_pv` is the opponent's best reply: in a stored review it is the next move's `best_pv`, in live it comes from the analysis of the position after the move. It is empty for the final move, and the reply facts are then omitted.

### Output

```rust
pub struct Digest {            // sketch; exact fields are settled in the plan
    pub mover: Side,
    pub san: String,
    pub class: MoveClass,      // as given, never re-judged
    pub loss: f64,
    pub eval_before: Eval,
    pub eval_after: Eval,
    pub best_san: Option<String>,
    pub played: MoveFacts,     // what the move did: capture, check, castle, promotion
    pub best: Option<MoveFacts>,
    pub reply_san: Option<String>,
    pub material: MaterialLines,   // now / end of played line / end of best line
    pub played_line: Vec<String>,  // SAN, 4-6 plies
    pub best_line: Vec<String>,
    pub facts: Vec<Fact>,          // ranked by significance, most important first
    pub depth: Option<u32>,
}
pub enum Fact { MateAllowed{..}, MateMissed{..}, MaterialLost{..}, Loose{..}, ForcedKingMove{..},
                AllowsFork{..}, AllowsPin{..},               // consequences for the mover
                ForcesMate{..}, WinsMaterial{..}, Forks{..}, Pins{..} }   // what the move achieves
```

Each fact is independent and optional. A PV move that does not replay drops only its own fact; if even the basics cannot be built, the app falls back to the old sentence. The digest is deterministic and unit-testable, and serialises to JSON (`chess-analyzer review --facts` prints it); milestone 2 renders the same digest into a prompt.

### Detectors

Each detector is a small pure function in its own file, tested on fixture positions. Adding a motif never touches the others. The first set, chosen by what explains real mistakes:

- **Material along the lines:** balance now, at the end of the played line and at the end of the best line.
- **Exchange-aware loose pieces:** pieces attacked and undefended, or attacked by something cheaper, *including batteries*. A plain attacker-versus-defender count is not enough: after 7.Qb3 the queen stands behind Bc4 on the diagonal to f7, and a count sees one attacker. This needs a real static-exchange evaluation.
- **King safety:** a check answered by a king move, lost castling rights. In the Opera Game, 6...Nf6 and the best move 6...Qf6 end with identical material, so material cannot explain the 13.8-point loss; the forced `Ke7` does.
- **Mates:** forced mate allowed, mate missed, a mate that was slower.
- **Motifs:** fork and pin first (a move that forks several pieces, or pins a piece worth at least a minor piece to the king; each also reported when it is the opponent's best reply). Back-rank weakness and discovered attack follow the same pattern later, one small function each.

## `core::commentary`

`render(&Digest) -> String` builds two or three plain sentences:

1. **Verdict:** "Nf6 is a mistake." A few phrasings per class, chosen by ply number so a game does not read identically; deterministic, so tests are exact.
2. **Cause:** the one or two highest-ranked facts, as a consequence of the move. For an error: "It allows Qb3, and after Bxf7+ Black's king is forced to e7 and Black can no longer castle." For a good move: what it achieves (wins a pawn, gives check, forces mate).
3. **Better move** (errors only): "Qf6 was better", plus what it does when that is a fact.

If the digest has no explanatory fact, the text says only what the engine shows: how the evaluation changed and which move was better. It never invents a cause. The text is deterministic and carries no depth notes.

All wording lives in this one module, in English. Pluralisation and translation (Fluent) are out of scope for now; keeping the strings in one place makes that later move cheap.

## Tauri app and UI

There is no new command. Commentary is a field of the review: `MoveReview.commentary: Option<String>` (serde default, so older JSON still loads), written by `review::review_move` from the two analyses it already holds: the position before the move, and the position after it, whose best line is the opponent's expected reply. So:

- a finished review has commentary on each move as soon as the move is classified, including while the review is still streaming in;
- a live move gets its commentary together with its class, and a new one whenever either changes (the live session already re-sends a move when anything about it changes);
- a review saved before this existed has no commentary on disk and gets it when it is loaded: `GameStore::get` calls `review::backfill_commentary`, which rebuilds the engine's reply to each move from the best line stored with the next move (the last move gets commentary without reply facts).

The UI shows `commentaryFor(move, ply)`: `move.commentary`, or the old `describeMove` sentence when there is none (a move still being analysed, or a position that could not be read).

- **Review screen:** the commentary line under the board.
- **Live screen:** a commentary line under the status text, for the selected move. A provisional move is already marked by its badge, so the line carries no extra note.
- A review never fails because of commentary.

## CLI

`chess-analyzer review game.pgn --commentary` prints the sentence under each critical move; `--facts` prints the ranked facts. This is the tuning tool: wording and detectors are checked against real games with it, as thresholds were tuned with the review CLI.

## Errors

- A detector that cannot run (a PV move that does not replay, a missing reply line) drops only its own fact.
- No usable fact: the evaluation-change sentence, honestly stated. The renderer never fills a gap with a guess.
- A move whose position or move cannot be read has `commentary: null`: the UI falls back to the old sentence.

## Testing

- **Detectors:** hand-built fixture positions for captures, loose pieces including a battery (the `f7` case), forced king moves, mates, material deltas, promotion, and a missing `reply_pv`.
- **Renderer:** each class, fact priority, the no-fact fallback, the last move, and a **grounding property test**: every piece, square and move named in the text appears in the digest, and the mover's colour is always the right one.
- **Golden commentary** for the two fixture games, in the style of the existing golden reviews, so wording and detector changes show up as readable diffs.
- **Pipeline:** a review's moves carry commentary; the live session's commentary matches a finished review's for the same analyses; a review saved without commentary gets it on load.
- **Frontend:** commentary shown for the selected move on both screens, fallback while a move has none.
- **End-to-end:** the existing review and live scripts also assert that a commentary sentence appears.
- The generated TypeScript drift check in CI covers the new types.

---

# Milestone 2: optional local-LLM coach

Built only after milestone 1 has been read on real games and judged too flat. It has its own plan. It adds a second renderer over the same `Digest`; nothing in milestone 1 changes shape.

## Behaviour

### The coach toggle

`llm.enabled` is the single switch, labelled "AI coach (local LLM)" in Settings, and it takes effect immediately. When it is **off**:

- **No connection, ever.** No provider object is built, no worker thread is started, and no HTTP request is made: not at startup, not when Settings opens, not on any screen. `check()` is never called.
- **No LLM UI.** The AI badge, Explain and Regenerate buttons, the "Explaining 3/9" chip and the status chip are not rendered. The commentary line is milestone 1's text. The coach's fields in Settings (URL, model, key, auto-explain, Test connection) are collapsed beneath the toggle.
- **Commands refuse.** `llm_status` returns `Disabled` without touching the network, `explain_*` return a `Disabled` error, and `explanations_lookup` returns nothing. The UI does not call them while the toggle is off.
- **Stored explanations are kept, not shown.** The cache rows stay in SQLite; turning the coach back on makes them visible again. (Off never shows AI text. Showing old text while off is a one-line change if preferred.)
- **Turning it off mid-run** drops queued explanations. A call already in flight finishes and is cached, but is not shown.

Turning it **on** does not probe the server. The user presses "Test connection", or the first Explain reports `Unreachable` if the server is not running. With the coach on but the server unreachable, milestone 1's text still covers every move.

### Review screen

The commentary line shows milestone 1's sentence immediately. LLM text replaces it with a small "AI" badge when it arrives, with a "Regenerate" action. Moves that are not critical show an "Explain" button. While the batch runs, a chip "Explaining 3/9" with a cancel is shown.

### Live screen

The commentary line keeps milestone 1's text and gains an "Explain" button. Pressing it calls `explain_move(Live, ply)`. Nothing is sent to the model otherwise: no timer, no automatic live call.

- The button works on any classified move, using that move's current (frozen or still-deepening) analysis.
- While the call runs the button shows a busy state. Moving on to another move does not cancel it; its text is cached and shown when the user returns to that move. A new move or take-back sends `cancel_explain("live")` to drop anything still queued.
- Pressing Explain again after the search has deepened produces a new explanation, because the digest differs.
- The model and Stockfish compete for CPU while the explanation is generated. The user chooses when, so this is their trade-off.
- The button is not rendered when the coach is off. On but unreachable, it stays visible and a press reports the error.

### Settings screen

A "Commentary" section: the coach toggle, base URL, model, optional API key, auto-explain toggle (reviews only), and a "Test connection" button showing the `llm_status` result. A one-line hint recommends a 7-8B instruct model and says smaller models may repeat the input or name the wrong side, without naming a specific model.

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

The system prompt is a versioned constant (`PROMPT_VERSION`). It states that the facts are engine ground truth; forbids naming any piece, square or move that is not in them; forbids re-judging the class or the best move; and fixes the shape (2-3 plain sentences, no markdown, no numbers beyond those given). It carries one or two short worked examples. The user message is the digest rendered as short tagged lines. One prompt is one move and a few hundred tokens; the whole game is never sent.

The reply is trimmed and stripped of stray markdown and quotes. If it repeats the prompt (small models do this: llama3.2 3B echoed the whole FACTS block in a manual test), everything up to the last `COMMENT:` marker is dropped. An empty reply, or one far over the length cap, is a failed call (`Empty` / `BadResponse`) and is not cached.

### Cache

```
explanations(key TEXT PRIMARY KEY, text, model, prompt_version, created_at)
key = SHA-256(prompt_version || model || system || user)
```

It lives in the same SQLite database as saved games, behind its own connection, like `GameStore` and `CachedAnalyzer`. Changing the model or the prompt misses the cache by construction, so nothing stale is shown. A live move whose analysis deepens produces a different digest and therefore a new key, and an older explanation remains a hit if the user goes back to it. "Regenerate" overwrites the row. If the database is unavailable, explanations work uncached with a warning.

### Service

`ExplainService` runs one worker thread with a queue, making one model call at a time. On-demand requests go to the front of the queue. It builds each `CommentaryInput` itself, never from UI-supplied engine data: `review::digest_for` for a saved game, and for a live game the analyses the live session holds (which would need a small new query on `LiveSession`). A cancel drops the queued items for a scope. A call already in flight cannot be interrupted (it is a blocking HTTP read); it finishes, its text is cached, and the UI ignores it if the key is stale.

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

## Commands and events

| Command | Purpose |
|---|---|
| `llm_status()` | Runs `check()`; returns reachable / model present / error. `Disabled` (no network) when the coach is off. |
| `explanations_lookup(source, plies)` | Cache read only. Never calls the model. Returns nothing while the coach is off. |
| `explain_move(source, ply, force)` | Returns `{ key, text? }` at once. If not cached, queues a call at the front. `force` regenerates. |
| `explain_critical(source)` | Queues every critical ply of a saved game not already cached. Started automatically when a review finishes (if `auto_explain`) and when a saved game is opened; cached plies cost nothing. |
| `cancel_explain(scope)` | Drops queued items for that scope. |

Events go out on the `explain-event` channel: `Ready { key, ply, text }`, `Failed { key, ply, error }`, `Progress { done, total }`. The UI keeps the latest `key` per move, drops events for keys it no longer wants, and looks text up by key (the same stale-event discipline as live revisions). All types derive `ts_rs::TS`.

## Errors

- **Coach off:** no calls and no connection; no LLM UI; stored text hidden.
- **Unreachable, timeout, HTTP error, empty or oversized reply:** a `Failed` event; the UI keeps milestone 1's text and offers "Couldn't generate: retry". Failures are never cached.
- **Batch:** one `Unreachable` stops the whole batch. Other per-move errors continue; three in a row abort it. A single summary is shown, not one message per move.
- **Database unavailable:** explanations work uncached, with a warning.
- **App closes mid-call:** nothing is persisted, because a row is written only after success.

## Testing

- **`narrator`:**
  - A `ScriptedProvider` fake for service tests: on-demand priority over the batch, skipping cached plies, cancel, the abort rules and stale keys.
  - Cache key sensitivity to model and prompt version.
  - `OpenAiCompatible` against a small stub HTTP server on a local port: success, 404, malformed JSON, timeout and `check()`.
  - Prompt snapshot tests, and echo stripping.
  - One integration test against a real local model, skipped when none is reachable (as with the Stockfish tests).
  - With the coach off, the service constructs no provider and a `ScriptedProvider` records zero calls.
- **Frontend (fake `Api`):** the milestone-1-to-AI swap and badge, regenerate, cached and failure states. With the coach off, no LLM element is rendered anywhere, no LLM `Api` method is called, and Settings shows only the toggle. The live Explain button sends exactly one request per press, shows busy and then the AI text, and is absent when the coach is off. The Settings section.
- **End-to-end:** `npm run e2e:explain` drives the real app against a stub OpenAI-compatible server, so it needs no model.
- **Tuning tool:** `chess-analyzer explain game.pgn [--dry-run]` prints each critical move's digest and the model's explanation; `--dry-run` prints only the prompts.

---

## Amends the live game spec

`2026-10-08-live-game-design.md` says "This feature has no LLM". With this work, live gains per-move commentary: milestone 1 shows template commentary for the selected move automatically (it needs no engine time), and milestone 2 adds an Explain button that sends the move to a local model, with nothing sent automatically. Live analyses are still not stored in SQLite; in milestone 2 only explanation text, keyed by prompt hash, is.

## Out of scope

- A game summary paragraph and a Q&A chat.
- Streaming responses.
- A content validator that rejects LLM explanations mentioning things outside the digest.
- An embedded llama.cpp and any model download or pull management.
- Keychain storage for the API key.
- Automatic LLM explanation of live moves (dropped; can be revisited if local models get fast enough).
- Translations and a template engine (Fluent); strategic or positional commentary beyond the detectors.
- Variations, as everywhere in v1.

## Left for the implementation plan

- Milestone 1 is planned in `docs/superpowers/plans/2026-10-09-move-commentary.md`. Milestone 2 is planned later, if the gate in "Decisions" is passed.
- Later detectors for milestone 1: back-rank weakness, discovered attack, and a fact for trades; the wording is tuned with `review --commentary` on real games.
- Milestone 2 only: prompt wording and examples, tuned against a real model (the user message should not end on a bare `COMMENT:` label, which made llama3.2 repeat the input); the `ureq` version and TLS feature; default `max_tokens`, temperature, length cap and timeouts; how `LiveSession` exposes the current analysis of a ply to the service.
