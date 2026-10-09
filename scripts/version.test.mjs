import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { checkVersions, readVersions, setVersion } from "./version.mjs";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const crate = (name) => `[[package]]\nname = "${name}"\nversion = "0.1.0"\n`;
const FILES = {
  "Cargo.toml": `[workspace]\nmembers = ["a"]\n\n[workspace.package]\nedition = "2024"\nversion = "0.1.0"\nrust-version = "1.88"\n`,
  "app/src-tauri/tauri.conf.json": `{\n  "productName": "Chess Analyzer",\n  "version": "0.1.0",\n  "bundle": { "active": true }\n}\n`,
  "app/package.json": `{\n  "name": "chess-analyzer-app",\n  "version": "0.1.0",\n  "dependencies": {\n    "react": "19.3.0"\n  }\n}\n`,
  "app/package-lock.json": `{\n  "name": "chess-analyzer-app",\n  "version": "0.1.0",\n  "lockfileVersion": 3,\n  "packages": {\n    "": {\n      "name": "chess-analyzer-app",\n      "version": "0.1.0"\n    },\n    "node_modules/react": {\n      "version": "19.3.0"\n    }\n  }\n}\n`,
  "Cargo.lock": [
    crate("chess-analyzer-app"),
    crate("chess-analyzer-cli"),
    crate("chess-analyzer-core"),
    crate("serde"),
  ].join("\n"),
};

function makeRoot(transform = (text) => text) {
  const root = mkdtempSync(join(tmpdir(), "version-test-"));
  for (const [file, text] of Object.entries(FILES)) {
    mkdirSync(dirname(join(root, file)), { recursive: true });
    writeFileSync(join(root, file), transform(text));
  }
  return root;
}

const read = (root, file) => readFileSync(join(root, file), "utf8");

test("every place the version is written is found and they agree", () => {
  const root = makeRoot();
  const { expected, problems } = checkVersions(root);
  assert.equal(expected, "0.1.0");
  assert.deepEqual(problems, []);
  assert.equal(readVersions(root).length, 8);
  rmSync(root, { recursive: true });
});

test("set rewrites every version field and nothing else", () => {
  const root = makeRoot();
  setVersion(root, "0.2.0");
  assert.deepEqual(checkVersions(root).problems, []);
  assert.equal(checkVersions(root).expected, "0.2.0");
  for (const [file, before] of Object.entries(FILES)) {
    assert.equal(read(root, file).split("\n").length, before.split("\n").length, file);
  }
  assert.match(read(root, "Cargo.lock"), /name = "serde"\nversion = "0.1.0"/);
  assert.match(read(root, "app/package-lock.json"), /"node_modules\/react": \{\n      "version": "19.3.0"/);
  assert.match(read(root, "Cargo.toml"), /rust-version = "1.88"/);
  rmSync(root, { recursive: true });
});

test("prerelease versions are accepted", () => {
  const root = makeRoot();
  setVersion(root, "0.1.0-rc1");
  assert.deepEqual(checkVersions(root, "v0.1.0-rc1").problems, []);
  rmSync(root, { recursive: true });
});

test("a bad version is refused and nothing is written", () => {
  const root = makeRoot();
  for (const bad of ["1.2", "v1.2.3", "1.2.3.4", ""]) {
    assert.throws(() => setVersion(root, bad), /not a version/);
  }
  for (const [file, before] of Object.entries(FILES)) assert.equal(read(root, file), before);
  rmSync(root, { recursive: true });
});

test("when one place cannot be found, nothing is written and the file is named", () => {
  const root = makeRoot();
  writeFileSync(join(root, "Cargo.lock"), FILES["Cargo.lock"].replace("chess-analyzer-core", "renamed"));
  assert.throws(() => setVersion(root, "0.2.0"), /Cargo\.lock/);
  assert.equal(read(root, "Cargo.toml"), FILES["Cargo.toml"]);
  rmSync(root, { recursive: true });
});

test("check names the file that disagrees", () => {
  const root = makeRoot();
  writeFileSync(
    join(root, "app/src-tauri/tauri.conf.json"),
    FILES["app/src-tauri/tauri.conf.json"].replace("0.1.0", "0.1.1"),
  );
  const { problems } = checkVersions(root);
  assert.equal(problems.length, 1);
  assert.match(problems[0], /tauri\.conf\.json/);
  rmSync(root, { recursive: true });
});

test("a tag must match the files", () => {
  const root = makeRoot();
  assert.deepEqual(checkVersions(root, "v0.1.0").problems, []);
  assert.equal(checkVersions(root, "v0.2.0").problems.length, 8);
  assert.equal(checkVersions(root, "v0.1.0-rc1").problems.length, 8);
  rmSync(root, { recursive: true });
});

test("files with Windows line endings work too", () => {
  const root = makeRoot((text) => text.replaceAll("\n", "\r\n"));
  setVersion(root, "0.3.0");
  assert.deepEqual(checkVersions(root).problems, []);
  assert.equal(checkVersions(root).expected, "0.3.0");
  rmSync(root, { recursive: true });
});

test("this repository's own files agree", () => {
  assert.deepEqual(checkVersions(REPO).problems, []);
});
