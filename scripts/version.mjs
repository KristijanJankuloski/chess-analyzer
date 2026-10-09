#!/usr/bin/env node
// Keeps the app's version in one agreed state.
//   node scripts/version.mjs check [vX.Y.Z]   fail if the files disagree (or disagree with the tag)
//   node scripts/version.mjs set X.Y.Z        rewrite every place the version is written
// Used by CI and by the release workflow.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;
const NL = String.raw`\r?\n`;
// A top-level `"version"` key, as written at two spaces of indentation in the JSON files.
const TOP_LEVEL = new RegExp(`(${NL}  "version": ")([^"]+)(")`);
const lockEntry = (crate) => new RegExp(`(name = "${crate}"${NL}version = ")([^"]+)(")`);

/** Every place the version is written. Each pattern captures: the text before it, the version, the text after it. */
export const SPOTS = [
  { file: "Cargo.toml", label: "workspace version", pattern: new RegExp(`(\\[workspace\\.package\\][\\s\\S]*?${NL}version = ")([^"]+)(")`) },
  { file: "app/src-tauri/tauri.conf.json", label: "Tauri version", pattern: TOP_LEVEL },
  { file: "app/package.json", label: "package version", pattern: TOP_LEVEL },
  { file: "app/package-lock.json", label: "lockfile version", pattern: TOP_LEVEL },
  {
    file: "app/package-lock.json",
    label: "lockfile root package",
    pattern: new RegExp(`("packages": \\{${NL}    "": \\{${NL}      "name": "chess-analyzer-app",${NL}      "version": ")([^"]+)(")`),
  },
  { file: "Cargo.lock", label: "Cargo.lock chess-analyzer-app", pattern: lockEntry("chess-analyzer-app") },
  { file: "Cargo.lock", label: "Cargo.lock chess-analyzer-cli", pattern: lockEntry("chess-analyzer-cli") },
  { file: "Cargo.lock", label: "Cargo.lock chess-analyzer-core", pattern: lockEntry("chess-analyzer-core") },
];

function missing(spot) {
  return new Error(`could not find the ${spot.label} in ${spot.file}`);
}

export function readVersions(root) {
  return SPOTS.map((spot) => {
    const match = spot.pattern.exec(readFileSync(join(root, spot.file), "utf8"));
    if (!match) throw missing(spot);
    return { ...spot, version: match[2] };
  });
}

export function checkVersions(root, tag) {
  const spots = readVersions(root);
  const expected = tag === undefined ? spots[0].version : tag.replace(/^v/, "");
  const problems = spots
    .filter((spot) => spot.version !== expected)
    .map((spot) => `${spot.file} (${spot.label}) says ${spot.version}, expected ${expected}`);
  return { expected, problems };
}

export function setVersion(root, version) {
  if (!SEMVER.test(version)) {
    throw new Error(`"${version}" is not a version like 1.2.3 or 1.2.3-rc1`);
  }
  // Work out every new file before writing any, so a pattern that no longer matches changes nothing.
  const files = new Map();
  for (const spot of SPOTS) {
    const text = files.get(spot.file) ?? readFileSync(join(root, spot.file), "utf8");
    if (!spot.pattern.test(text)) throw missing(spot);
    files.set(spot.file, text.replace(spot.pattern, (_all, before, _old, after) => `${before}${version}${after}`));
  }
  for (const [file, text] of files) writeFileSync(join(root, file), text);
}

function main([command, argument]) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  try {
    if (command === "check") {
      const { expected, problems } = checkVersions(root, argument);
      if (problems.length > 0) {
        console.error(problems.join("\n"));
        process.exit(1);
      }
      console.log(`Every version field says ${expected}.`);
    } else if (command === "set" && argument) {
      setVersion(root, argument);
      console.log(`Version set to ${argument}. Review the diff, then commit.`);
    } else {
      console.error("usage: node scripts/version.mjs check [vX.Y.Z] | set X.Y.Z");
      process.exit(2);
    }
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
