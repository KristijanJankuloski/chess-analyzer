# Windows release Design

Date: 2026-10-09
Status: Draft for review

## Intent

Ship the first public build of Chess Analyzer: a Windows installer on GitHub Releases that a user can download, install without admin rights and use without touching a terminal. Releases are cut by pushing a version tag, so the process is repeatable and does not depend on one person's machine.

Success: a Windows user opens the release page, runs `Chess Analyzer_x.y.z_x64-setup.exe`, launches the app, clicks one button to get Stockfish, and reviews a game. The maintainer cuts a release by bumping the version in a PR, merging, and pushing a tag.

Constraints carried over from `AGENTS.md`: no cloud, no account, analysis fully offline. The Stockfish download is the app's only network use and happens once, on request.

## Decisions made during brainstorming

| Topic | Decision |
|---|---|
| Audience | Windows users, installer. Linux and macOS are deferred: their Stockfish setup script has never been run and macOS needs notarization. |
| Stockfish | **Not bundled.** The app offers an in-app "Download Stockfish" button. This keeps the installer small and avoids shipping GPLv3 code in the MIT project's installer. It resolves the "bundled vs. downloaded on first run" open decision in `AGENTS.md` for Windows. |
| Installer | NSIS `.exe`, per-user install (no admin prompt). |
| Publishing | CI builds a **draft** GitHub Release; the maintainer reads it and publishes by hand. |
| Signing, auto-update | Out of scope for the first release. The README tells users to expect a Windows SmartScreen warning. |

## 1. Getting Stockfish

New module `crates/core/src/engine_install.rs`, using `ureq` (blocking HTTP, matching the sync core) and `zip`.

- The Stockfish version, the asset name and its SHA-256 are constants in the module. The hash is computed from the real asset when this is implemented; it is never guessed.
- Steps: download to a temp file, verify the SHA-256, extract `stockfish*.exe` to `<app_data>/engines/stockfish.exe`, run the UCI handshake. Success is reported only after the handshake answers `uciok`.
- A failure at any step (offline, hash mismatch, bad archive, failed handshake) returns a plain-language error and leaves no partial exe behind.
- `locate_stockfish` does not change. On success the app saves the installed path into `Settings.engine_path`, so the Settings field shows where the engine is and the existing "Check engine" flow applies unchanged. Manual paths and `STOCKFISH_PATH` keep working.
- Tauri command `download_stockfish` runs on a blocking thread and emits progress events, as review jobs do. The download is in Rust, so the CSP is unchanged.
- UI: when `EngineStatus.found` is false, the Settings screen shows a "Download Stockfish 19" button with a progress bar and a retry on failure. The home screen shows a one-line banner linking to it when no engine is found. `app/src/api/fake.ts` gets a matching stub.

## 2. Release pipeline

- `tauri.conf.json`: `bundle.active: true`, target `nsis`, per-user install. The Stockfish GPLv3 notice is included in the installer metadata.
- `scripts/bump-version` takes a version and updates the workspace `Cargo.toml`, `tauri.conf.json`, `app/package.json` and the lockfiles together. CI gains a cheap check that all of them agree.
- `.github/workflows/release.yml`, triggered by `v*` tags:
  1. Run the same checks as `ci.yml`.
  2. Fail if the tag does not match the version in the files.
  3. Build with `tauri-action` on `windows-latest`.
  4. Create a draft GitHub Release with the installer and a `.sha256` file; release notes are generated from merged PRs. Tags containing `-` (such as `v0.1.0-rc1`) create a pre-release.
- README: an "Install" section (download, SmartScreen note, first-run Stockfish download) and a "Releasing" checklist: bump version in a PR, merge, `git tag vX.Y.Z && git push --tags`, review the draft, publish.
- The README license section is updated: the installer does not contain Stockfish, and the in-app download fetches it from the official Stockfish release.

## 3. Testing

- Unit tests (no network) for archive extraction, hash mismatch, partial-file cleanup and asset naming, using a tiny fixture zip.
- Handshake verification reuses the existing Stockfish integration-test path, which prints `SKIPPED` when no binary is found.
- Frontend tests for the Settings download states (idle, progress, failed, done) and the home banner, through the fake API.
- A version-agreement check in CI.
- Manual, once: push `v0.1.0-rc1`, install the resulting pre-release on a clean Windows profile, download Stockfish, and review a game. Only then tag `v0.1.0`.

## Out of scope

Code signing, auto-update, Linux and macOS builds, bundling Stockfish, choosing an engine variant by CPU capability (the `universal` asset is used, as `scripts/setup-stockfish.ps1` does today).

## Open questions

- Which Stockfish version to pin: the README and setup script use `sf_19`; confirm it is still the right one when implementing.
