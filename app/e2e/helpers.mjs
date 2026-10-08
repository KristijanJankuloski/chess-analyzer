// Shared plumbing for the end-to-end scripts: attach to the running desktop app's webview.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";

const here = path.dirname(fileURLToPath(import.meta.url));
export const repoRoot = path.resolve(here, "../..");
export const outDir = path.join(here, "out");
fs.mkdirSync(outDir, { recursive: true });

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
export const log = (...parts) => console.log(new Date().toISOString().slice(11, 19), ...parts);

/** Connects to the app started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222. */
export async function connect(port = 9222) {
  const browser = await puppeteer.connect({
    browserURL: `http://127.0.0.1:${port}`,
    defaultViewport: null,
  }).catch((error) => {
    throw new Error(
      `Could not attach to the app on port ${port}. Start it first:\n` +
        `  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=${port} npm run tauri dev\n(${error.message})`,
    );
  });
  const pages = await browser.pages();
  const page = pages.find((p) => /localhost|tauri/.test(p.url())) ?? pages[0];
  const errors = [];
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`console: ${m.text()}`);
  });
  await page.waitForSelector(".app__nav", { timeout: 30000 });

  const clickText = (text) =>
    page.evaluate((t) => {
      const button = [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === t);
      if (!button) throw new Error(`no button "${t}"`);
      button.click();
    }, text);

  /** Sets a React-controlled text field the way typing would. */
  const setValue = (selector, value) =>
    page.evaluate(
      (sel, text) => {
        const el = document.querySelector(sel);
        const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
        Object.getOwnPropertyDescriptor(proto.prototype, "value").set.call(el, text);
        el.dispatchEvent(new Event("input", { bubbles: true }));
      },
      selector,
      value,
    );

  const shot = (name) => page.screenshot({ path: path.join(outDir, `${name}.png`) });
  const finish = async () => {
    await browser.disconnect();
    if (errors.length) {
      console.error("PAGE ERRORS:\n" + errors.join("\n"));
      process.exitCode = 1;
    } else {
      log("no page errors");
    }
  };
  return { page, clickText, setValue, shot, finish };
}

export function fail(message) {
  console.error(`FAILED: ${message}`);
  process.exitCode = 1;
}
