// End-to-end check of the review flow in the real desktop app, with the real Stockfish:
// engine check in Settings, a streamed review, the saved history, and a refused PGN.
//
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run tauri dev   (one terminal)
//   npm run e2e:review                                                                      (another)
import fs from "node:fs";
import path from "node:path";
import { connect, fail, log, repoRoot } from "./helpers.mjs";

const pgn = fs.readFileSync(path.join(repoRoot, "data/fixtures/opera_game.pgn"), "utf8");
const { page, clickText, setValue, shot, finish } = await connect();

// Settings: a quick depth, and check that the engine can be found and started.
await clickText("Settings");
await page.waitForSelector("#setting-depth");
await setValue("#setting-depth", "12");
await clickText("Check engine");
await page.waitForFunction(
  () => /Found Stockfish|not found/.test(document.body.innerText),
  { timeout: 20000 },
);
const engine = await page.evaluate(() => document.body.innerText.match(/Found Stockfish[^\n]*|[^\n]*not found[^\n]*/)?.[0]);
log("engine:", engine);
if (!engine?.startsWith("Found Stockfish")) fail(`engine not found: ${engine}`);
await clickText("Games");

// Review the Opera Game and watch it stream in.
await page.waitForSelector("#pgn-text");
await setValue("#pgn-text", pgn);
const started = Date.now();
await clickText("Review");
await page.waitForSelector(".banner", { timeout: 15000 });
log("progress banner:", await page.evaluate(() => document.querySelector(".banner")?.textContent));
await page.waitForFunction(
  () => !document.querySelector(".banner") && document.querySelector('[aria-label="White accuracy"]')?.textContent !== "–",
  { timeout: 240000, polling: 500 },
);
log(`review finished in ${((Date.now() - started) / 1000).toFixed(1)}s`);
const accuracy = await page.evaluate(() => [...document.querySelectorAll(".summary__accuracy")].map((e) => e.textContent));
const opening = await page.evaluate(() => document.querySelector(".summary__opening")?.textContent);
log("accuracy:", accuracy.join(" / "), "| opening:", opening);
if (accuracy.includes("–") || !opening?.includes("Philidor")) fail("the finished review is missing its accuracy or opening");
await shot("review-complete");

// It is saved: back to the list, reopen it.
await clickText("← Back");
await page.waitForSelector(".home__game");
const games = await page.evaluate(() => [...document.querySelectorAll(".home__game")].map((e) => e.textContent));
log("history:", games[0]?.slice(0, 70));
if (!games.some((g) => g.includes("Paul Morphy"))) fail("the game is not in the history");
await page.click(".home__game");
await page.waitForSelector(".review__body");

// A PGN with no game in it is refused with a readable message.
await clickText("← Back");
await page.waitForSelector("#pgn-text");
await setValue("#pgn-text", "this is not chess");
await clickText("Review");
await page.waitForSelector('[role="alert"]');
const refusal = await page.evaluate(() => document.querySelector('[role="alert"]').textContent);
log("refusal:", refusal);
if (!refusal.includes("no game found")) fail(`unexpected refusal message: ${refusal}`);

await finish();
