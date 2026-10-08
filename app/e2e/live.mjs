// End-to-end check of the live game tab in the real desktop app: type the moves of a game as
// they would arrive from a broadcast, and check that the real Stockfish answers with an
// evaluation, a deepening line, a class badge on every move and the two arrows, that the
// game survives leaving the tab, and that a finished game ends the analysis cleanly.
//
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 npm run tauri dev   (one terminal)
//   npm run e2e:live                                                                       (another)
import { connect, fail, log, sleep } from "./helpers.mjs";

const { page, clickText, setValue, shot, finish } = await connect();

/** Types a move into the box, as someone following a game would. */
const enter = async (text) => {
  await page.click("#live-san");
  await page.type("#live-san", text);
  await page.keyboard.press("Enter");
  await sleep(150);
};

const moveLabels = () =>
  page.evaluate(() =>
    [...document.querySelectorAll(".move")].map((e) => e.getAttribute("aria-label") ?? "").filter(Boolean),
  );

const depthReached = () =>
  page.evaluate(() => {
    const text = document.querySelector(".live-lines__depth")?.textContent ?? "";
    return Number.parseInt(text.replace(/\D+/g, ""), 10) || 0;
  });

// The board draws each arrow as a path stroked in the colour BoardPanel gives it.
const GREEN_ARROW = "rgba(129, 182, 76, 0.92)";
const RED_ARROW = "rgba(202, 52, 49, 0.88)";
const arrows = (color) =>
  page.evaluate((c) => document.querySelectorAll(`.board-panel svg path[stroke="${c}"]`).length, color);

await clickText("Live");
await page.waitForSelector("#live-san");

// Start from an empty board even if an earlier run left a game behind.
if ((await moveLabels()).length > 0) {
  await clickText("New game");
  await clickText("Discard game");
  await sleep(300);
}

// 1. f3 e5: the engine must answer for the newest position quickly.
await enter("f3");
await enter("e5");
await page.waitForFunction(
  () => document.querySelector(".eval-bar__label")?.textContent?.trim() !== "…",
  { timeout: 30000, polling: 250 },
);
log("evaluation shown:", await page.evaluate(() => document.querySelector(".eval-bar__label").textContent));

await page.waitForFunction(
  () => /depth\s+(1[2-9]|[2-9]\d)/.test(document.querySelector(".live-lines__depth")?.textContent ?? ""),
  { timeout: 60000, polling: 250 },
);
const first = await depthReached();
log("deepening, at depth", first);
await sleep(3000);
const later = await depthReached();
log("and later at depth", later);
if (later <= first) fail(`the search did not keep deepening (${first} then ${later})`);

// 2. g4 is a blunder (it allows mate in one): the badge and the red arrow must say so.
await enter("g4");
await page.waitForFunction(
  () => [...document.querySelectorAll(".move")].some((e) => /^g4, Blunder/.test(e.getAttribute("aria-label") ?? "")),
  { timeout: 60000, polling: 250 },
);
const labels = await moveLabels();
log("moves:", labels.join(" | "));
const [green, red] = [await arrows(GREEN_ARROW), await arrows(RED_ARROW)];
log("arrows on the board: green", green, "red", red);
if (green !== 1) fail(`expected one green arrow (the best next move), saw ${green}`);
if (red !== 1) fail(`expected one red arrow (the move that should have been played), saw ${red}`);

// Hiding the engine's current best move leaves only the arrow for the move just played.
const toggle = () => page.click(".nav-controls__toggle input");
await toggle();
const [greenOff, redOff] = [await arrows(GREEN_ARROW), await arrows(RED_ARROW)];
log("with the current best move hidden: green", greenOff, "red", redOff);
if (greenOff !== 0 || redOff !== 1) fail(`expected only the red arrow, saw green ${greenOff} red ${redOff}`);
await toggle();
if ((await arrows(GREEN_ARROW)) !== 1) fail("the green arrow should come back");
const lines = await page.evaluate(() =>
  [...document.querySelectorAll(".live-lines__line")].map((e) => e.textContent),
);
log("engine lines:", lines.join(" | "));
if (!lines.some((l) => l.includes("Qh4#"))) fail("the engine's lines should show Black's mate Qh4#");
await shot("live-blunder");

// 3. Leaving the tab pauses the analysis; coming back keeps the game and resumes it.
await clickText("Games");
await page.waitForSelector("#pgn-text");
await sleep(500);
await clickText("Live");
await page.waitForSelector("#live-san");
const back = await moveLabels();
if (back.length !== 3) fail(`the game should still have 3 moves after leaving, found ${back.length}`);
await page.waitForFunction(
  () => /depth\s+\d+/.test(document.querySelector(".live-lines__depth")?.textContent ?? ""),
  { timeout: 30000, polling: 250 },
);

// 3b. Changing an engine setting gives the live analysis a fresh engine with the new setting.
await clickText("Settings");
await page.waitForSelector("#setting-multipv");
await setValue("#setting-multipv", "2");
await clickText("Save");
await page.waitForFunction(() => document.body.innerText.includes("Saved."), { timeout: 10000 });
await clickText("Live");
await page.waitForSelector("#live-san");
await page.waitForFunction(() => document.querySelectorAll(".live-lines__line").length === 2, {
  timeout: 60000,
  polling: 250,
});
log("two engine lines after setting MultiPV to 2");

// 4. Taking back and entering the mating move ends the game and the analysis.
await enter("Qh4#");
await page.waitForFunction(
  () => [...document.querySelectorAll('[role="status"]')].some((e) => e.textContent.includes("Checkmate: 0-1")),
  { timeout: 30000, polling: 250 },
);
const disabled = await page.evaluate(() => document.querySelector("#live-san").disabled);
if (!disabled) fail("the move box should be disabled once the game is over");
await page.waitForFunction(
  () => /Game over/.test(document.querySelector(".live-lines__title")?.textContent ?? ""),
  { timeout: 30000, polling: 250 },
);
await shot("live-finished");

// 5. The finished game goes to the normal review.
await clickText("Review this game");
await page.waitForSelector(".banner", { timeout: 15000 });
await page.waitForFunction(
  () => !document.querySelector(".banner") && document.querySelector('[aria-label="White accuracy"]')?.textContent !== "–",
  { timeout: 240000, polling: 500 },
);
log("reviewed:", await page.evaluate(() => document.querySelector("h1").textContent));

// Put the setting back, so running this script leaves the app as it found it.
await clickText("Settings");
await page.waitForSelector("#setting-multipv");
await setValue("#setting-multipv", "3");
await clickText("Save");
await page.waitForFunction(() => document.body.innerText.includes("Saved."), { timeout: 10000 });

await finish();
