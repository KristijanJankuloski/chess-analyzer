// Real reviews produced by the Rust core (see crates/core/tests/golden.rs), typed for the UI.
// Each came from its own throwaway store, so both are saved as game 1; give them distinct ids.
import type { StoredGame } from "../generated/StoredGame";
import foolsMateJson from "./fools_mate.stored.json";
import operaGameJson from "./opera_game.stored.json";

function withId(json: unknown, id: number): StoredGame {
  const stored = json as StoredGame;
  return { ...stored, summary: { ...stored.summary, id } };
}

export const foolsMate = withId(foolsMateJson, 1);
export const operaGame = withId(operaGameJson, 2);
