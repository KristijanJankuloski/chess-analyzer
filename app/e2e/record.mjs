// End-to-end check of record-by-hand in the real desktop app: enter a game by clicking the
// board (with a capturing promotion, en passant and castling), review it with the real
// Stockfish, then check that a game that ends on the board locks its result.
//
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run tauri dev   (one terminal)
//   npm run e2e:record                                                                      (another)
import { connect, fail, log, sleep } from "./helpers.mjs";

const game = [
  "h2h4", "g7g5", "h4g5", "g8f6", "g5g6", "a7a6", "g6g7", "a6a5", "g7f8q", "e8f8", "g1f3",
  "b8c6", "e2e4", "c6b4", "e4e5", "d7d5", "e5d6", "b4c6", "f1c4", "f8g7", "e1g1",
];
const { page, clickText, shot, finish } = await connect();

const play = async (moves) => {
  for (const move of moves) {
    await page.click(`[data-square="${move.slice(0, 2)}"]`);
    await page.click(`[data-square="${move.slice(2, 4)}"]`);
    if (move.length === 5) {
      await page.waitForSelector('[role="group"][aria-label="Promote to"]');
      await clickText("Queen");
    }
    await sleep(120);
  }
};

await clickText("Record");
await page.waitForSelector('[data-square="e2"]');
await page.type("#record-white", "Me");
await page.type("#record-black", "Practice partner");
await play(game);
const moves = await page.evaluate(() =>
  [...document.querySelectorAll(".move")].map((e) => e.getAttribute("aria-label") ?? "").filter(Boolean),
);
log("recorded:", moves.join(" "));
if (moves.length !== game.length) fail(`expected ${game.length} moves on the board, found ${moves.length}`);
if (!moves.includes("O-O") || !moves.some((m) => m.startsWith("gxf8=Q")) || !moves.includes("exd6")) {
  fail("castling, promotion or en passant was not recorded");
}
await shot("record-entered");

// The real Rust core and Stockfish must accept exactly what the board allowed.
await clickText("Review this game");
await page.waitForSelector(".banner", { timeout: 15000 });
await page.waitForFunction(
  () => !document.querySelector(".banner") && document.querySelector('[aria-label="White accuracy"]')?.textContent !== "–",
  { timeout: 240000, polling: 500 },
);
const title = await page.evaluate(() => document.querySelector("h1").textContent);
const accuracy = await page.evaluate(() => [...document.querySelectorAll(".summary__accuracy")].map((e) => e.textContent));
log("reviewed:", title, "| accuracy:", accuracy.join(" / "));
if (!title.includes("Me vs Practice partner")) fail(`unexpected title ${title}`);
await shot("record-reviewed");

// A game that ends on the board (fool's mate) fixes its own result and takes no more moves.
await clickText("Record");
await page.waitForSelector('[data-square="e2"]');
await play(["f2f3", "e7e5", "g2g4", "d8h4"]);
const result = await page.evaluate(() => {
  const select = document.querySelector("#record-result");
  return { value: select.value, disabled: select.disabled };
});
log("fool's mate result:", JSON.stringify(result));
if (result.value !== "0-1" || !result.disabled) fail("the result of a finished game should be fixed at 0-1");

await finish();
