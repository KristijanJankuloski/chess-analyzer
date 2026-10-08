# Live game mode Design

Date: 2026-10-08
Status: Draft for review

## Intent

The user watches a game somewhere else (a tournament broadcast, a game over the board) and enters its moves, for both White and Black, as they happen. The app shows a Chess.com-style review that **updates live**: an evaluation bar, the engine's best move as an arrow, the top lines, and a class badge on every move entered so far. When the game ends, it can be handed to the existing full review.

Success: after each move is entered, the bar and the best-move arrow are on screen within about a second and keep improving while the user waits for the next move. Everything stays local; nothing is fetched from the internet.

Core principle (from `AGENTS.md`) is unchanged: Stockfish is the authority on chess. This feature has no LLM.

## Decisions made during brainstorming

| Topic | Decision |
|---|---|
| Who plays | Nobody in the app. The user enters both sides' moves. No engine opponent, no strength setting. |
| Entry | Click or drag on the board, or type SAN (`Nf3`, `O-O`, `exd5`) into a box. Take-back is supported. |
| Class badges | Every entered move gets a class badge. The latest move's badge is **provisional** while the search is still deepening; earlier badges are frozen when the next move arrives. |
| Arrows | Green: the engine's best move for the side to move now. Red: the move the last mover should have played, only when their move was an inaccuracy, mistake, miss or blunder. |
| Engine approach | One persistent Stockfish that is told to analyse the live position until the next move arrives. Rejected: one-shot fixed-depth runs per move (nothing updates during a long think); Stockfish in the browser (breaks the native-engine decision). |
| Joining mid-game | When several moves arrive at once, only the last position gets the deep analysis. Earlier positions get a quick shallow pass, so every move still has a provisional badge. |
| Depth | The live position is searched with no depth limit until the next move, take-back or pause. The depth setting continues to apply to the full review only. |
| Leaving the tab | Pauses the search and keeps the engine process idle with its analyses; coming back resumes. The move list is also saved in the browser, so an app restart does not lose a game. |
| Persistence of live analyses | Not stored in SQLite. They are provisional by design; the full review is the saved record. |

## Structure

No new crates. The work lands in the existing three places:

- `crates/core`: a search-streaming engine seam, a `LiveSession` that owns the engine thread, and `LiveEvent` types.
- `app/src-tauri`: three commands and one event channel, a thin shell over `core`.
- `app/src`: a Live screen that reuses the Record screen's move entry and the Review screen's board, bar, graph and move list.

## `core` crate

### Streaming search (`engine.rs`)

`Analyzer::analyze` blocks until a depth limit is reached, so it cannot serve a search that runs until interrupted. A second trait is added beside it:

```rust
pub enum SearchLimit { Depth(u32), Infinite }

pub trait LiveEngine {
    /// Starts searching `fen`. Any search in progress is stopped first.
    fn start(&mut self, fen: &str, multipv: u32, limit: SearchLimit) -> Result<(), EngineError>;
    /// The next update, or `None` if nothing arrived within `wait`.
    fn poll(&mut self, wait: Duration) -> Result<Option<SearchUpdate>, EngineError>;
    /// Stops the current search and waits until the engine is idle.
    fn stop(&mut self) -> Result<(), EngineError>;
}

pub enum SearchUpdate {
    /// All lines of one completed depth, sorted by rank.
    Depth(PositionAnalysis),
    /// A bounded search ended.
    Finished,
}
```

`UciEngine` implements it with `go depth N` / `go infinite`, and `stop`. After `stop` it drains output up to `bestmove` and then synchronises with `isready` / `readyok`. Without that, the old search's `bestmove` can arrive after the next `go` and be taken for its result. This is the one place in the feature where a subtle bug is likely, so it gets its own tests against real Stockfish.

Lines are grouped per depth: an update is published only when every requested line has reported at that depth, so the lines on screen never mix depths. `ScriptedLiveEngine` (tests) replays a list of updates per position.

### Session (`live.rs`)

`LiveSession` owns one engine and one background thread. The thread is driven by messages:

- `SetMoves(Vec<String>)`: the complete move list in UCI, sent each time it changes.
- `Pause`, `Shutdown`.

On `SetMoves` the session:

1. Replays the moves with `Game::from_uci_moves`. An illegal move is reported as `LiveEvent::Error` and the previous state is kept.
2. Keeps the stored analysis of every position that is unchanged from before (the common prefix). Positions after the first difference are discarded.
3. Freezes the previous live position's analysis at the depth it reached.
4. Decides what to search, in this order:
   - the **live position** (the last one) to a quick first answer (`Depth(QUICK_DEPTH)`), so the bar moves at once;
   - every earlier position that has no analysis yet, newest first, at `Depth(BACKLOG_DEPTH)`;
   - the live position again with `Infinite`, until the next message.
5. A new message always interrupts the current search; the order is then recomputed. Terminal positions (checkmate, stalemate, insufficient material) are never sent to the engine; `review::terminal_analysis` supplies their evaluation.

`QUICK_DEPTH` is 12 and `BACKLOG_DEPTH` is 12; both are constants in `live.rs`, not user settings. MultiPV, threads and hash come from `Settings`.

### Classification

Move `i` is classified with the existing `review::review_move`, made `pub(crate)`, from `analyses[i]` and `analyses[i + 1]`. This keeps one definition of every class, including the `material_swing` and `prev_opponent_class` inputs and the opening-book rule. A move is classified as soon as both its analyses exist and is re-classified every time the position after it deepens, until the next move freezes it.

### Events

```rust
pub enum LiveEvent {
    /// Analysis of position `index` (0 = the start) improved, or was frozen.
    Position { revision: u64, index: usize, depth: u32, lines: Vec<LiveLine>, frozen: bool },
    /// Move `ply` was (re)classified.
    Move { revision: u64, review: MoveReview, provisional: bool },
    /// The engine failed. The session keeps the move list.
    Error { revision: u64, message: String },
}
```

`LiveLine` is `AnalysisLine` plus `pv_san: Vec<String>`, so the UI never converts moves itself. `revision` is bumped by every `SetMoves`; the UI drops events with an older revision, so a late event from a stopped search can never overwrite newer state. All types derive `ts_rs::TS` into `app/src/generated/`.

## Tauri app and UI

### Commands and events

| Command | Purpose |
|---|---|
| `live_update(moves: Vec<String>) -> u64` | Starts the session on first use, sends `SetMoves`, returns the revision. |
| `live_pause()` | Stops the search; the engine and analyses stay. |
| `live_reset()` | Shuts the session down (the "New game" action). |

Events go out on the `live-event` channel. The three functions are added to the single `Api` interface with a Tauri implementation and a scriptable fake, as for the other screens. The session is dropped when the app closes, which kills Stockfish (the existing `Drop`).

### Live screen

- Board with the eval bar and a "depth N" label; green and red arrows as described above.
- Move entry: the Record screen's click/drag logic (`lib/record.ts`: `tryMove`, `takeBack`, `isPromotion`) extracted into a shared hook, plus a SAN box that resolves the typed move against the current position with chess.js and shows "Illegal move: Nf6" inline without clearing the box.
- Move list with class badges (provisional badges are visually distinct), the eval graph (the existing component, filling in as moves arrive) and running accuracy for both players, computed from the classified moves exactly as the review does.
- Top lines: up to MultiPV lines, each with its evaluation and the first moves in SAN.
- Optional names and a result field, as in Record mode. "Review this game" sends the game to the existing review through `toReviewSource`.
- Navigation: stepping back through the list shows the arrows and badge for that move from its frozen analysis. Entering a move while stepping back is refused until the user returns to the end.
- The move list is saved to browser storage on every change (all access wrapped in try/catch) and restored on the next start.
- A new "Live" entry in the existing top-level navigation, next to Record.

## Errors

- Illegal typed or clicked move: refused in the UI; the backend also re-validates and reports `LiveEvent::Error` without changing state.
- Engine missing: the same "Stockfish was not found" message and Settings link used by the other screens.
- Engine dies or times out: the session restarts the process once and re-searches the live position; if that fails, `LiveEvent::Error` is shown with a "Restart analysis" button. The move list is never lost.
- The game ends (checkmate, stalemate, insufficient material): the final position gets its terminal evaluation and the search stops.

## Testing

- `core`: `LiveSession` with `ScriptedLiveEngine` covers: one move; a batch of moves (deep search only on the last, shallow backlog first); a take-back and re-entry (common prefix kept); a new move arriving mid-backlog; stale revisions; an illegal move list; a terminal position never reaching the engine; provisional-to-frozen transition of a badge.
- Real Stockfish integration tests: `start` then `stop` then `start` never mixes up a late `bestmove`; an infinite search yields increasing depths; MultiPV lines at one depth arrive together.
- Frontend: Live screen against the fake `Api` (moves, SAN entry, errors, provisional badges, arrows, resume after leaving, restore from storage).
- A puppeteer end-to-end script (`npm run e2e:live`) enters a short game in the real app and checks that the bar, badges and arrows appear.
- The generated TypeScript drift check in CI covers the new types.

## Out of scope

- Fetching live games from Chess.com, Lichess or a broadcast feed (the app stays offline).
- Playing against Stockfish.
- Saving live analyses to SQLite or reusing them as the full review's cache.
- Variations: the move list is one line, as everywhere in v1.
- Clocks and time-per-move.
- Making `QUICK_DEPTH` and `BACKLOG_DEPTH` user-configurable.

## Open items to settle during planning

- Whether the shared move-entry hook should also replace the logic inside the Record screen, or only be used by Live (risk of regressing Record versus duplicated code).
- Throttling of `Position` events if deep searches produce more updates than the UI needs.
- Whether running accuracy should be shown while the game is in progress; it moves a lot early on.
