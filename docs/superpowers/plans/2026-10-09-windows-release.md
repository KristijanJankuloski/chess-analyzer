# Windows Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Chess Analyzer as a per-user Windows installer on GitHub Releases, built by CI from a version tag, with an in-app button that downloads Stockfish.

**Architecture:** A new core module (`engine_install`) downloads the pinned Stockfish zip, checks its SHA-256, unpacks the engine, proves it answers the UCI handshake, then moves it into `<app data>/engines/`. A thin Tauri command runs it, streams progress events and saves the path into `Settings.engine_path`, so `locate_stockfish` does not change. The release is a `v*`-tag-triggered workflow that reuses `ci.yml`, checks the tag against the version in the files, builds an NSIS installer and drafts a GitHub Release.

**Tech Stack:** Rust (`ureq` 3.4, `zip` 9.0, `sha2` 0.11 in core), Tauri 2 NSIS bundler, React + TypeScript + vitest, Node (`node:test`) for the version script, GitHub Actions + `gh`.

**Spec:** `docs/superpowers/specs/2026-10-09-windows-release-design.md`

## Deviations from the spec (flag these when reviewing)

1. **No `tauri-action`.** The workflow runs `npm run tauri build -- --bundles nsis` and `gh release create --draft --generate-notes`. Same result, two fewer moving parts to debug.
2. **GPL notice lives next to the download, not in the installer.** The installer contains no Stockfish, so the GPL attaches where Stockfish is fetched: the in-app notice beside the button, plus the README. (`nsis.license` may require RTF and would show the MIT text anyway, so it is not used.)
3. **The releasing checklist is `docs/releasing.md`**, linked from the README, to keep the README for users.
4. **Download supports 64-bit Windows only** (the installer is x64 only). Other platforms get a clear "download it yourself" message.
5. **The Settings screen now checks the engine when it opens** (the spec's "shown when `found` is false" needs a status before anyone presses Check engine).

## Global Constraints

- Windows installer only: NSIS, per-user install (`installMode: currentUser`, no admin prompt).
- Stockfish is **not bundled**; the in-app download uses a pinned version, asset name and SHA-256 held as constants.
- `locate_stockfish` does not change; on success the app saves the installed path into `Settings.engine_path`.
- Releases are **draft** GitHub Releases from `v*` tags; tags containing `-` (such as `v0.1.0-rc1`) are pre-releases.
- The version must agree in `Cargo.toml`, `app/src-tauri/tauri.conf.json`, `app/package.json`, `app/package-lock.json` and `Cargo.lock`.
- The Stockfish download is the app's only network use; analysis stays fully offline; the CSP stays unchanged (the download is done in Rust).
- Out of scope: code signing, auto-update, Linux and macOS, bundling Stockfish, CPU-variant selection (the `universal` asset is used).
- Repo rules: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, and the generated TypeScript in `app/src/generated` must be committed and current (CI fails on a `git diff`).
- Commits: end each message with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` (use a second `-m`).
- Work on branch `feature/windows-release` (the spec is already committed there).

## Review Focus

Failure modes the spec implies that a user is most likely to hit. Each has a named test in the owning task.

1. **Connection drops or the user is offline mid-download** → a plain error, nothing half-installed, no scratch files left. (Task 1: `a_dropped_connection_is_reported_and_leaves_nothing_behind`.)
2. **The download is not the pinned release** (GitHub error page, corrupt zip, wrong hash) → refused and discarded. (Task 1: `a_checksum_mismatch_installs_nothing`, `something_that_is_not_an_archive_is_refused`.)
3. **A failed reinstall must not destroy a working engine.** (Task 1: `a_failed_handshake_keeps_the_engine_that_was_already_there`.)
4. **Download pressed twice, or the user leaves Settings and comes back mid-download** → one download at a time. (Task 1: `only_one_install_can_run_at_a_time`; Task 3: button disabled while running.)
5. **Unsaved edits in Settings survive the download.** (Task 3: `keeps_edits_that_were_not_saved_yet`.)
6. **A tag that does not match the files must not produce a release.** (Task 5: tag-mismatch test.)
7. **A crashed earlier download leaves a scratch folder** → the next attempt still works. (Task 1: `a_leftover_scratch_folder_does_not_get_in_the_way`.)

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/core/src/engine_install.rs` (new) | Pinned release constants, download → verify → unpack → handshake → install, progress type, single-flight lock |
| `crates/core/src/lib.rs` | `pub mod engine_install;` |
| `crates/core/Cargo.toml` | `ureq`, `zip`, `sha2` |
| `crates/core/tests/stockfish.rs` | Handshake integration tests + an ignored real-download test |
| `app/src-tauri/src/lib.rs` | `download_stockfish` command, `install-progress` event, `AppState` gets `engines_dir` and `install_lock` |
| `app/src/api/types.ts`, `tauri.ts`, `fake.ts` | `downloadStockfish`, `onInstallProgress` on the API and the fake |
| `app/src/lib/install.ts` (new) | Wording and bar fraction for install progress |
| `app/src/screens/SettingsScreen.tsx` | Engine check on open, download button, progress, GPL notice |
| `app/src/screens/HomeScreen.tsx`, `app/src/App.tsx` | "Stockfish not found" banner leading to Settings |
| `app/src/styles.css` | Styles for the two new blocks |
| `scripts/version.mjs`, `scripts/version.test.mjs` (new) | One-command version bump and check |
| `app/src-tauri/tauri.conf.json` | NSIS bundling on, per-user |
| `.github/workflows/ci.yml`, `release.yml` (new) | `workflow_call`, version check; tag-driven release |
| `README.md`, `docs/releasing.md` (new), `AGENTS.md` | Install and release docs; resolve the distribution open decision |

---

### Task 1: Stockfish installer module (core)

**Files:**
- Create: `crates/core/src/engine_install.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml`, `crates/core/tests/stockfish.rs`
- Generated (commit them): `app/src/generated/InstallProgress.ts`, `app/src/generated/Installed.ts`

**Interfaces:**
- Consumes: `crate::engine::{Analyzer, EngineConfig, EngineError, UciEngine}` (`UciEngine::start(EngineConfig::new(PathBuf))`, `Analyzer::engine_id(&self) -> String`).
- Produces (used by Task 2):
  - `pub fn install_stockfish(engines_dir: &Path, progress: &mut dyn FnMut(InstallProgress)) -> Result<Installed, InstallError>`
  - `pub enum InstallProgress { Downloading { downloaded: u64, total: Option<u64> }, Verifying, Installing }` (serde tag `"stage"`, snake_case)
  - `pub struct Installed { pub path: String, pub engine: String }`
  - `pub struct InstallLock` with `Default` and `try_acquire(&self) -> Option<InstallPermit<'_>>`
  - `pub enum InstallError` (`Display` gives the user-facing message)
  - `pub fn verify_handshake(path: &Path) -> Result<String, InstallError>`

- [ ] **Step 1: Add the dependencies and the module with its declarations and tests**

In `crates/core/Cargo.toml`, add under `[dependencies]` (keep the list alphabetical):

```toml
sha2 = "0.11"
ureq = "3.4"
zip = { version = "9.0", default-features = false, features = ["deflate"] }
```

In `crates/core/src/lib.rs`, add `pub mod engine_install;` after `pub mod engine;`.

Create `crates/core/src/engine_install.rs` with the declarations and the tests only. The functions come in Step 3, so this does not compile yet.

````rust
//! Gets Stockfish onto the machine for users who do not have it: downloads the pinned official
//! release, checks its SHA-256, unpacks the engine, proves it answers the UCI handshake and only
//! then puts it in place. UI-agnostic, like the rest of core; the desktop shell forwards the
//! progress it reports.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use ts_rs::TS;

use crate::engine::{Analyzer, EngineConfig, EngineError, UciEngine};

/// The release this build downloads. To move to a newer Stockfish, change the tag and asset name
/// and set the hash to `sha256sum` of the asset from the release page.
pub const STOCKFISH_TAG: &str = "sf_19";
pub const ASSET_NAME: &str = "stockfish-windows-x86-64-universal.zip";
pub const ASSET_SHA256: &str = "3c8bf1f9ea66a09350a40df4f632288285ac206d99f33ab5842c408fc30b48a7";

/// Scratch folder inside the engines directory. Removed on every outcome.
const WORK_DIR: &str = ".installing";
/// The engine's file name once installed.
const ENGINE_FILE: &str = "stockfish.exe";
/// Progress is reported each time this many more bytes have arrived.
const REPORT_EVERY: u64 = 256 * 1024;

pub fn asset_url() -> String {
    format!("https://github.com/official-stockfish/Stockfish/releases/download/{STOCKFISH_TAG}/{ASSET_NAME}")
}

/// How far an install has got, for a progress bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum InstallProgress {
    Downloading {
        #[ts(type = "number")]
        downloaded: u64,
        #[ts(type = "number | null")]
        total: Option<u64>,
    },
    /// Checking the download, unpacking it and starting the engine once.
    Verifying,
    Installing,
}

/// What a finished install produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Installed {
    /// Where the engine is now.
    pub path: String,
    /// The engine's own name and version, e.g. "Stockfish 19".
    pub engine: String,
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error(
        "Automatic download only works on 64-bit Windows. Download Stockfish from stockfishchess.org and enter its path above."
    )]
    Unsupported,
    #[error("could not download Stockfish: {0}")]
    Download(String),
    #[error("the download is not the expected Stockfish release (checksum mismatch), so it was discarded")]
    ChecksumMismatch,
    #[error("could not unpack Stockfish: {0}")]
    Archive(String),
    #[error("the downloaded engine does not work: {0}")]
    Engine(#[from] EngineError),
    #[error("could not write Stockfish to disk: {0}")]
    Io(#[from] io::Error),
}

/// Allows one install at a time. `try_acquire` hands out a permit that frees the lock when it is
/// dropped, including when the install panics.
#[derive(Default)]
pub struct InstallLock(AtomicBool);

pub struct InstallPermit<'a>(&'a AtomicBool);

impl InstallLock {
    pub fn try_acquire(&self) -> Option<InstallPermit<'_>> {
        (!self.0.swap(true, Ordering::SeqCst)).then(|| InstallPermit(&self.0))
    }
}

impl Drop for InstallPermit<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn hex<'a>(bytes: impl Iterator<Item = &'a u8>) -> String {
    bytes.map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const ENGINE_ENTRY: &str = "stockfish/stockfish-windows-x86-64-universal.exe";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "chess-analyzer-install-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut bytes));
            let options = zip::write::SimpleFileOptions::default();
            for (name, content) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(content).unwrap();
            }
            writer.finish().unwrap();
        }
        bytes
    }

    fn engine_zip() -> Vec<u8> {
        zip_with(&[("stockfish/Copying.txt", b"GPL"), (ENGINE_ENTRY, b"engine bytes")])
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        hex(Sha256::digest(bytes).iter())
    }

    fn ok_engine(_: &Path) -> Result<String, InstallError> {
        Ok("Stockfish 19".to_string())
    }

    fn install(
        body: Vec<u8>,
        sha: &str,
        dir: &Path,
        verify: &dyn Fn(&Path) -> Result<String, InstallError>,
        seen: &mut Vec<InstallProgress>,
    ) -> Result<(String, PathBuf), InstallError> {
        let total = Some(body.len() as u64);
        install_from(Cursor::new(body), total, sha, dir, verify, &mut |p| seen.push(p))
    }

    #[test]
    fn the_download_comes_from_the_official_release() {
        assert_eq!(
            asset_url(),
            "https://github.com/official-stockfish/Stockfish/releases/download/sf_19/stockfish-windows-x86-64-universal.zip"
        );
    }

    #[test]
    fn the_pinned_checksum_is_a_lowercase_sha256() {
        assert_eq!(ASSET_SHA256.len(), 64);
        assert!(ASSET_SHA256.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn the_checksum_is_computed_while_copying() {
        let mut out = Vec::new();
        let digest =
            copy_hashed(&mut Cursor::new(b"abc".to_vec()), &mut out, Some(3), &mut |_| {}).unwrap();
        assert_eq!(digest, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(out, b"abc");
    }

    #[test]
    fn progress_only_grows_and_ends_at_the_full_size() {
        let mut seen = Vec::new();
        copy_hashed(
            &mut Cursor::new(vec![7u8; 1_000_000]),
            &mut Vec::new(),
            Some(1_000_000),
            &mut |p| seen.push(p),
        )
        .unwrap();
        let sizes: Vec<u64> = seen
            .iter()
            .map(|p| match p {
                InstallProgress::Downloading { downloaded, .. } => *downloaded,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert!(sizes.windows(2).all(|w| w[0] < w[1]), "{sizes:?}");
        assert_eq!(sizes.last(), Some(&1_000_000));
        assert!(sizes.len() < 20, "reported in chunks, not on every read: {}", sizes.len());
    }

    #[test]
    fn the_engine_is_found_inside_the_archive_and_nothing_else_is_unpacked() {
        let dir = temp_dir("extract");
        let archive = dir.join("a.zip");
        fs::write(
            &archive,
            zip_with(&[
                ("stockfish/Copying.txt", b"GPL"),
                ("stockfish/src/stockfish-tool.exe", b"not the engine"),
                (ENGINE_ENTRY, b"engine bytes"),
            ]),
        )
        .unwrap();
        let target = dir.join("out.exe");
        extract_engine(&archive, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"engine bytes");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2, "only the archive and the engine");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_without_an_engine_is_refused() {
        let dir = temp_dir("no-engine");
        let archive = dir.join("a.zip");
        fs::write(&archive, zip_with(&[("stockfish/Copying.txt", b"GPL")])).unwrap();
        let err = extract_engine(&archive, &dir.join("out.exe")).unwrap_err();
        assert!(matches!(err, InstallError::Archive(_)), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn something_that_is_not_an_archive_is_refused() {
        let dir = temp_dir("not-a-zip");
        let body = b"<html>Not Found</html>".to_vec();
        let err = install(body.clone(), &sha256_hex(&body), &dir, &ok_engine, &mut Vec::new())
            .unwrap_err();
        assert!(matches!(err, InstallError::Archive(_)), "{err}");
        assert!(!dir.join(ENGINE_FILE).exists());
        assert!(!dir.join(WORK_DIR).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_good_download_is_installed_and_the_scratch_space_removed() {
        let dir = temp_dir("installs");
        let zip = engine_zip();
        let mut seen = Vec::new();
        let (name, path) =
            install(zip.clone(), &sha256_hex(&zip), &dir, &ok_engine, &mut seen).unwrap();
        assert_eq!(name, "Stockfish 19");
        assert_eq!(path, dir.join(ENGINE_FILE));
        assert_eq!(fs::read(&path).unwrap(), b"engine bytes");
        assert!(!dir.join(WORK_DIR).exists());
        let stages: Vec<_> = seen
            .iter()
            .filter(|p| !matches!(p, InstallProgress::Downloading { .. }))
            .collect();
        assert_eq!(stages, [&InstallProgress::Verifying, &InstallProgress::Installing]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_checksum_mismatch_installs_nothing() {
        let dir = temp_dir("mismatch");
        let err = install(engine_zip(), &"0".repeat(64), &dir, &ok_engine, &mut Vec::new())
            .unwrap_err();
        assert!(matches!(err, InstallError::ChecksumMismatch), "{err}");
        assert!(!dir.join(ENGINE_FILE).exists());
        assert!(!dir.join(WORK_DIR).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_handshake_keeps_the_engine_that_was_already_there() {
        let dir = temp_dir("handshake");
        fs::write(dir.join(ENGINE_FILE), b"old engine").unwrap();
        let zip = engine_zip();
        let broken = |_: &Path| -> Result<String, InstallError> {
            Err(InstallError::Engine(EngineError::Protocol("no uciok".into())))
        };
        let err = install(zip.clone(), &sha256_hex(&zip), &dir, &broken, &mut Vec::new())
            .unwrap_err();
        assert!(matches!(err, InstallError::Engine(_)), "{err}");
        assert_eq!(fs::read(dir.join(ENGINE_FILE)).unwrap(), b"old engine");
        assert!(!dir.join(WORK_DIR).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn installing_again_replaces_the_old_engine() {
        let dir = temp_dir("replace");
        fs::write(dir.join(ENGINE_FILE), b"old engine").unwrap();
        let zip = engine_zip();
        install(zip.clone(), &sha256_hex(&zip), &dir, &ok_engine, &mut Vec::new()).unwrap();
        assert_eq!(fs::read(dir.join(ENGINE_FILE)).unwrap(), b"engine bytes");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_leftover_scratch_folder_does_not_get_in_the_way() {
        let dir = temp_dir("leftover");
        fs::create_dir_all(dir.join(WORK_DIR)).unwrap();
        fs::write(dir.join(WORK_DIR).join("junk"), b"from a crashed run").unwrap();
        let zip = engine_zip();
        install(zip.clone(), &sha256_hex(&zip), &dir, &ok_engine, &mut Vec::new()).unwrap();
        assert!(!dir.join(WORK_DIR).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    struct DropsAfter(usize);

    impl Read for DropsAfter {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Err(io::Error::new(io::ErrorKind::ConnectionReset, "connection reset"));
            }
            let n = buf.len().min(self.0);
            buf[..n].fill(1);
            self.0 -= n;
            Ok(n)
        }
    }

    #[test]
    fn a_dropped_connection_is_reported_and_leaves_nothing_behind() {
        let dir = temp_dir("dropped");
        let err = install_from(
            DropsAfter(100_000),
            Some(1_000_000),
            &"0".repeat(64),
            &dir,
            &ok_engine,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(&err, InstallError::Download(m) if m.contains("connection reset")), "{err}");
        assert!(!dir.join(ENGINE_FILE).exists());
        assert!(!dir.join(WORK_DIR).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_one_install_can_run_at_a_time() {
        let lock = InstallLock::default();
        let first = lock.try_acquire();
        assert!(first.is_some());
        assert!(lock.try_acquire().is_none());
        drop(first);
        assert!(lock.try_acquire().is_some());
    }
}
````

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p chess-analyzer-core engine_install`
Expected: FAIL to compile with "cannot find function `copy_hashed`" (and `extract_engine`, `install_from`).

- [ ] **Step 3: Write the implementation**

Insert the following into `crates/core/src/engine_install.rs`, between `fn hex(...)` and `#[cfg(test)] mod tests`:

````rust
/// Copies `reader` to `writer` and returns the SHA-256 (lowercase hex) of what was copied,
/// reporting progress as it goes. A failure to read is a download problem; a failure to write is
/// a disk problem.
fn copy_hashed(
    reader: &mut impl Read,
    writer: &mut impl Write,
    total: Option<u64>,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<String, InstallError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut downloaded = 0u64;
    let mut reported = 0u64;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(InstallError::Download(e.to_string())),
        };
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
        downloaded += read as u64;
        if downloaded - reported >= REPORT_EVERY {
            reported = downloaded;
            progress(InstallProgress::Downloading { downloaded, total });
        }
    }
    if reported != downloaded {
        progress(InstallProgress::Downloading { downloaded, total });
    }
    Ok(hex(hasher.finalize().iter()))
}

/// Unpacks the Stockfish executable from the release archive to `target`. The archive also holds
/// the engine's source and docs; only the executable is taken.
fn extract_engine(archive_path: &Path, target: &Path) -> Result<(), InstallError> {
    let archive_error = |e: &dyn std::fmt::Display| InstallError::Archive(e.to_string());
    let mut archive =
        zip::ZipArchive::new(File::open(archive_path)?).map_err(|e| archive_error(&e))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| archive_error(&e))?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let in_sources = path.components().any(|c| c.as_os_str() == "src");
        if entry.is_file() && file_name.starts_with("stockfish") && file_name.ends_with(".exe") && !in_sources
        {
            io::copy(&mut entry, &mut File::create(target)?)?;
            return Ok(());
        }
    }
    Err(InstallError::Archive("no Stockfish program in the download".into()))
}

/// Starts the engine once to prove it is a working Stockfish; returns its name.
pub fn verify_handshake(path: &Path) -> Result<String, InstallError> {
    let engine = UciEngine::start(EngineConfig::new(path.to_path_buf()))?;
    let name = engine.engine_id();
    if name.starts_with("Stockfish") {
        Ok(name)
    } else {
        Err(InstallError::Engine(EngineError::Protocol(format!(
            "expected Stockfish, but the engine calls itself {name}"
        ))))
    }
}

/// Installs from a stream: saves it, checks it against `expected_sha256`, unpacks the engine,
/// runs `verify` on it and only then moves it to `<engines_dir>/stockfish.exe`, replacing any
/// engine already there. Whatever happens, the scratch folder is removed and an existing engine
/// is left alone unless the new one is proven. Returns the engine's name and path.
fn install_from(
    mut body: impl Read,
    total: Option<u64>,
    expected_sha256: &str,
    engines_dir: &Path,
    verify: &dyn Fn(&Path) -> Result<String, InstallError>,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<(String, PathBuf), InstallError> {
    let work = engines_dir.join(WORK_DIR);
    // A crashed earlier run may have left its scratch folder behind.
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)?;
    let result = (|| {
        let archive = work.join(ASSET_NAME);
        let actual = copy_hashed(&mut body, &mut File::create(&archive)?, total, progress)?;
        progress(InstallProgress::Verifying);
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            return Err(InstallError::ChecksumMismatch);
        }
        let unpacked = work.join(ENGINE_FILE);
        extract_engine(&archive, &unpacked)?;
        let name = verify(&unpacked)?;
        progress(InstallProgress::Installing);
        let target = engines_dir.join(ENGINE_FILE);
        fs::rename(&unpacked, &target)?;
        Ok((name, target))
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

/// Downloads the pinned Stockfish release into `engines_dir` and makes sure it works.
pub fn install_stockfish(
    engines_dir: &Path,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<Installed, InstallError> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(InstallError::Unsupported);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_global(Some(Duration::from_secs(30 * 60)))
        .build()
        .into();
    let response = agent
        .get(&asset_url())
        .call()
        .map_err(|e| InstallError::Download(e.to_string()))?;
    let total = response.body().content_length();
    let (engine, path) = install_from(
        response.into_body().into_reader(),
        total,
        ASSET_SHA256,
        engines_dir,
        &verify_handshake,
        progress,
    )?;
    Ok(Installed {
        path: path.to_string_lossy().into_owned(),
        engine,
    })
}
````

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `cargo test -p chess-analyzer-core engine_install`
Expected: PASS (14 hand-written tests, plus the `export_bindings_*` tests that ts-rs generates). If `cargo fmt` complains later, run `cargo fmt --all`.

- [ ] **Step 5: Add the integration tests against a real engine**

Append to `crates/core/tests/stockfish.rs`, and add `use chess_analyzer_core::engine_install::{InstallProgress, install_stockfish, verify_handshake};` to its imports:

```rust
#[test]
fn the_installer_accepts_a_real_engine() {
    let Some(path) = locate_stockfish(None) else {
        eprintln!("SKIPPED: Stockfish not found");
        return;
    };
    let name = verify_handshake(&path).expect("a real Stockfish passes the handshake");
    assert!(name.starts_with("Stockfish"), "{name}");
}

#[test]
fn the_installer_refuses_a_file_that_is_not_an_engine() {
    let path = std::env::temp_dir().join(format!(
        "chess-analyzer-not-an-engine-{}.exe",
        std::process::id()
    ));
    std::fs::write(&path, b"this is not a program").unwrap();
    assert!(verify_handshake(&path).is_err());
    let _ = std::fs::remove_file(&path);
}

/// Run on purpose: `cargo test -p chess-analyzer-core --test stockfish installs_the_real_release -- --ignored`
#[test]
#[ignore = "downloads about 81 MB from GitHub"]
fn installs_the_real_release() {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "chess-analyzer-real-install-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut last = None;
    let installed =
        install_stockfish(&dir, &mut |p| last = Some(p)).expect("the real release installs");
    assert!(installed.engine.starts_with("Stockfish"), "{}", installed.engine);
    assert!(std::path::Path::new(&installed.path).is_file());
    assert_eq!(last, Some(InstallProgress::Installing));
    let _ = std::fs::remove_dir_all(&dir);
}
```

Run: `cargo test -p chess-analyzer-core --test stockfish the_installer`
Expected: PASS (the real-engine one prints `SKIPPED` if there is no `engines/stockfish.exe`; run `scripts/setup-stockfish.ps1` first to exercise it).

- [ ] **Step 6: Prove the pinned release really installs**

Run: `cargo test -p chess-analyzer-core --test stockfish installs_the_real_release -- --ignored --nocapture`
Expected: PASS after downloading about 81 MB. If it fails with `ChecksumMismatch`, the constants are wrong: recompute with `sha256sum` of the asset and fix `ASSET_SHA256`.

- [ ] **Step 7: Format, lint, generate types, and commit**

```bash
cargo fmt --all
cargo clippy -p chess-analyzer-core --all-targets -- -D warnings
cargo test -p chess-analyzer-core
git status --short
```
Expected: clippy clean; tests pass; `git status` shows the new `app/src/generated/InstallProgress.ts` and `Installed.ts`. Open them and confirm `InstallProgress` is `{ "stage": "downloading", downloaded: number, total: number | null, } | { "stage": "verifying", } | { "stage": "installing", }`.

```bash
git add crates/core app/src/generated Cargo.lock
git commit -m "Add the Stockfish installer to core" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `download_stockfish` command (desktop shell)

**Files:**
- Modify: `app/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes (Task 1): `install_stockfish`, `Installed`, `InstallLock::try_acquire`.
- Produces (used by Task 3): Tauri command `download_stockfish` (no arguments) resolving to `Installed` (`{ path, engine }`) or rejecting with a message string; event `install-progress` carrying an `InstallProgress` payload.

The shell has no unit tests by design (see the file's header comment); this task is checked by compiling with clippy and by Task 9's manual run.

- [ ] **Step 1: Add the command, the state and the event**

In `app/src-tauri/src/lib.rs`:

1. Add the import next to the other core imports:
```rust
use chess_analyzer_core::engine_install::{InstallLock, Installed, install_stockfish};
```
2. Change `use std::path::Path;` to `use std::path::{Path, PathBuf};`.
3. Under `const LIVE_EVENT: &str = "live-event";` add:
```rust
const INSTALL_EVENT: &str = "install-progress";
```
4. Add two fields to `AppState`:
```rust
    /// Where a downloaded Stockfish is kept (inside the app's data directory).
    engines_dir: PathBuf,
    install_lock: InstallLock,
```
5. In `run()`, in the `app.manage(AppState { ... })` call, add:
```rust
                engines_dir: dir.join("engines"),
                install_lock: InstallLock::default(),
```
6. Add the command after `check_engine_status`:
```rust
/// Downloads Stockfish into the app's data directory and points the settings at it. The download
/// is large, so it runs off the main thread, and a second request while one is running is refused.
#[tauri::command(async)]
fn download_stockfish(state: State<'_, AppState>) -> Result<Installed, String> {
    let Some(_permit) = state.install_lock.try_acquire() else {
        return Err("Stockfish is already being downloaded.".into());
    };
    let handle = state.handle.clone();
    let installed = install_stockfish(&state.engines_dir, &mut |progress| {
        if let Err(e) = handle.emit(INSTALL_EVENT, &progress) {
            eprintln!("could not send {INSTALL_EVENT}: {e}");
        }
    })
    .map_err(message)?;
    let mut settings = state.settings.lock().map_err(message)?;
    settings.engine_path = Some(installed.path.clone());
    state.settings_file.save(&settings).map_err(message)?;
    Ok(installed)
}
```
7. Register it in `tauri::generate_handler![ ... ]` after `check_engine_status,`:
```rust
            download_stockfish,
```

- [ ] **Step 2: Build the frontend (the Tauri build embeds it) and compile**

```bash
cd app && npm ci && npm run build && cd ..
cargo clippy -p chess-analyzer-app --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: clippy clean, format clean.

- [ ] **Step 3: Commit**

```bash
git add app/src-tauri/src/lib.rs
git commit -m "Add the download_stockfish command" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Download button in Settings (frontend)

**Files:**
- Modify: `app/src/api/types.ts`, `app/src/api/tauri.ts`, `app/src/api/fake.ts`, `app/src/screens/SettingsScreen.tsx`, `app/src/screens/SettingsScreen.test.tsx`, `app/src/styles.css`
- Create: `app/src/lib/install.ts`, `app/src/lib/install.test.ts`

**Interfaces:**
- Consumes (Tasks 1-2): generated `InstallProgress` and `Installed` types; command `download_stockfish`; event `install-progress`.
- Produces (used by Task 4): `Api.checkEngine()` is already there; this task adds `Api.downloadStockfish(): Promise<Installed>` and `Api.onInstallProgress(handler): Promise<() => void>`, plus on `FakeApi`: `emitInstall(progress)` and options `install?: Installed`, `installError?: string`.

- [ ] **Step 1: Write the failing tests for the progress wording**

Create `app/src/lib/install.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { describeInstall, installFraction } from "./install";

describe("describeInstall", () => {
  it("says it is starting before any news arrives", () => {
    expect(describeInstall(null)).toBe("Starting…");
  });

  it("shows megabytes downloaded against the total", () => {
    expect(describeInstall({ stage: "downloading", downloaded: 20_000_000, total: 80_000_000 })).toBe(
      "Downloading… 20 of 80 MB",
    );
  });

  it("shows only what has arrived when the size is unknown", () => {
    expect(describeInstall({ stage: "downloading", downloaded: 5_400_000, total: null })).toBe(
      "Downloading… 5 MB",
    );
  });

  it("names the later stages", () => {
    expect(describeInstall({ stage: "verifying" })).toBe("Checking the download…");
    expect(describeInstall({ stage: "installing" })).toBe("Installing…");
  });
});

describe("installFraction", () => {
  it("is the share downloaded, never above one", () => {
    expect(installFraction({ stage: "downloading", downloaded: 25, total: 100 })).toBe(0.25);
    expect(installFraction({ stage: "downloading", downloaded: 120, total: 100 })).toBe(1);
  });

  it("is unknown (an indeterminate bar) before the size is known", () => {
    expect(installFraction(null)).toBeNull();
    expect(installFraction({ stage: "downloading", downloaded: 5, total: null })).toBeNull();
  });

  it("is full once the download is done", () => {
    expect(installFraction({ stage: "verifying" })).toBe(1);
    expect(installFraction({ stage: "installing" })).toBe(1);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd app && npx vitest run src/lib/install.test.ts`
Expected: FAIL, "Failed to resolve import ./install".

- [ ] **Step 3: Implement `install.ts`**

Create `app/src/lib/install.ts`:

```ts
import type { InstallProgress } from "../generated/InstallProgress";

const MB = 1_000_000;

/** What to tell the user while Stockfish downloads. `null` means no news has arrived yet. */
export function describeInstall(progress: InstallProgress | null): string {
  if (!progress) return "Starting…";
  switch (progress.stage) {
    case "downloading": {
      const done = Math.round(progress.downloaded / MB);
      return progress.total
        ? `Downloading… ${done} of ${Math.round(progress.total / MB)} MB`
        : `Downloading… ${done} MB`;
    }
    case "verifying":
      return "Checking the download…";
    case "installing":
      return "Installing…";
  }
}

/** How full the progress bar is (0 to 1), or null when unknown (an indeterminate bar). */
export function installFraction(progress: InstallProgress | null): number | null {
  if (!progress) return null;
  if (progress.stage !== "downloading") return 1;
  return progress.total ? Math.min(1, progress.downloaded / progress.total) : null;
}
```

Run: `cd app && npx vitest run src/lib/install.test.ts` → Expected: PASS.

- [ ] **Step 4: Extend the API and the fake**

`app/src/api/types.ts`: add imports `import type { InstallProgress } from "../generated/InstallProgress";` and `import type { Installed } from "../generated/Installed";`, and inside `Api` after `checkEngine()`:

```ts
  /**
   * Downloads Stockfish and points the settings at it. Progress arrives through
   * `onInstallProgress`. Rejects with a message if it could not be downloaded or does not work.
   */
  downloadStockfish(): Promise<Installed>;
  /** Subscribes to download progress; resolves to a function that unsubscribes. */
  onInstallProgress(handler: (progress: InstallProgress) => void): Promise<() => void>;
```

`app/src/api/tauri.ts`: add the type imports for `InstallProgress`, then

```ts
/** The event name for Stockfish download progress (see `INSTALL_EVENT` in src-tauri). */
export const INSTALL_EVENT = "install-progress";
```
and inside `tauriApi` after `checkEngine`:
```ts
  downloadStockfish: () => invoke("download_stockfish"),
  onInstallProgress: (handler) =>
    listen<InstallProgress>(INSTALL_EVENT, (event) => handler(event.payload)),
```

`app/src/api/fake.ts`: add type imports for `InstallProgress` and `Installed`; to `FakeApi` add
```ts
  /** Delivers a download progress event to every subscriber. */
  emitInstall(progress: InstallProgress): void;
```
to `FakeOptions` add
```ts
  /** What `downloadStockfish` resolves to. */
  install?: Installed;
  /** Make `downloadStockfish` reject with this message. */
  installError?: string;
```
in `createFakeApi` add `const installHandlers = new Set<(progress: InstallProgress) => void>();`, `emitInstall: (progress) => installHandlers.forEach((handler) => handler(progress)),` next to `emitLive`, and after `checkEngine`:
```ts
    downloadStockfish: async () => {
      record("downloadStockfish");
      if (options.installError) throw options.installError;
      const installed = options.install ?? { path: "C:/data/engines/stockfish.exe", engine: "Stockfish 19" };
      settings = { ...settings, engine_path: installed.path };
      return installed;
    },
    onInstallProgress: async (handler) => {
      record("onInstallProgress");
      installHandlers.add(handler);
      return () => {
        installHandlers.delete(handler);
      };
    },
```

- [ ] **Step 5: Write the failing Settings tests**

In `app/src/screens/SettingsScreen.test.tsx`: change the first import to `import { act, render, screen, waitFor } from "@testing-library/react";`, add `import type { EngineStatus } from "../generated/EngineStatus";` and `import type { Installed } from "../generated/Installed";`.

Two **existing** tests change because the screen now also checks the engine when it opens:

In "checks the engine after saving the path, and reports what it found" replace
`expect(names.indexOf("saveSettings")).toBeLessThan(names.indexOf("checkEngine"));` with
`expect(names.indexOf("saveSettings")).toBeLessThan(names.lastIndexOf("checkEngine"));`

In "does not check the engine when the settings could not be saved" replace
`expect(api.calls.some((c) => c[0] === "checkEngine")).toBe(false);` with
```ts
    // Only the check made when the screen opened.
    expect(api.calls.filter((c) => c[0] === "checkEngine")).toHaveLength(1);
```

Append a new block at the end of the file:

```tsx
const MISSING: EngineStatus = {
  found: false,
  name: null,
  error: "Stockfish was not found; run scripts/setup-stockfish or set the engine path",
};

describe("SettingsScreen, getting Stockfish", () => {
  it("offers no download when the engine works", async () => {
    await setup();
    expect(await screen.findByText("Found Stockfish 19")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Download Stockfish/ })).not.toBeInTheDocument();
  });

  it("checks the engine when it opens and offers the download if it is missing", async () => {
    await setup({ engine: MISSING });
    expect(await screen.findByRole("button", { name: "Download Stockfish 19" })).toBeEnabled();
    expect(screen.getByText(/GNU GPL/)).toBeInTheDocument();
  });

  it("downloads, shows the engine as found and fills in the path", async () => {
    const { api, user } = await setup({ engine: MISSING });
    await user.click(await screen.findByRole("button", { name: "Download Stockfish 19" }));
    expect(await screen.findByText("Found Stockfish 19")).toBeInTheDocument();
    expect(screen.getByLabelText("Stockfish path")).toHaveValue("C:/data/engines/stockfish.exe");
    expect(api.calls).toContainEqual(["downloadStockfish"]);
    expect(screen.queryByRole("button", { name: /Download Stockfish/ })).not.toBeInTheDocument();
  });

  it("shows progress while it downloads and does not allow a second click", async () => {
    const { api, user } = await setup({ engine: MISSING });
    let finish!: (installed: Installed) => void;
    api.downloadStockfish = () =>
      new Promise<Installed>((resolve) => {
        finish = resolve;
      });
    await user.click(await screen.findByRole("button", { name: "Download Stockfish 19" }));
    expect(screen.getByRole("button", { name: "Download Stockfish 19" })).toBeDisabled();
    expect(screen.getByText("Starting…")).toBeInTheDocument();

    act(() => api.emitInstall({ stage: "downloading", downloaded: 20_000_000, total: 80_000_000 }));
    expect(screen.getByText("Downloading… 20 of 80 MB")).toBeInTheDocument();
    act(() => api.emitInstall({ stage: "verifying" }));
    expect(screen.getByText("Checking the download…")).toBeInTheDocument();

    await act(async () => finish({ path: "C:/e/stockfish.exe", engine: "Stockfish 19" }));
    expect(await screen.findByText("Found Stockfish 19")).toBeInTheDocument();
  });

  it("explains a failed download and lets the user try again", async () => {
    const { user } = await setup({
      engine: MISSING,
      installError: "could not download Stockfish: connection reset",
    });
    await user.click(await screen.findByRole("button", { name: "Download Stockfish 19" }));
    expect(await screen.findByText("could not download Stockfish: connection reset")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download Stockfish 19" })).toBeEnabled();
  });

  it("keeps edits that were not saved yet", async () => {
    const { user } = await setup({ engine: MISSING });
    const depth = screen.getByLabelText("Depth");
    await user.clear(depth);
    await user.type(depth, "14");
    await user.click(await screen.findByRole("button", { name: "Download Stockfish 19" }));
    await screen.findByText("Found Stockfish 19");
    expect(screen.getByLabelText("Depth")).toHaveValue(14);
  });
});
```

- [ ] **Step 6: Run to verify failure**

Run: `cd app && npx vitest run src/screens/SettingsScreen.test.tsx`
Expected: FAIL (no "Download Stockfish 19" button; engine not checked on open).

- [ ] **Step 7: Implement the Settings changes**

In `app/src/screens/SettingsScreen.tsx`:

1. Imports: add `import type { InstallProgress } from "../generated/InstallProgress";` and `import { describeInstall, installFraction } from "../lib/install";`.
2. After the `message` state add:
```tsx
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [downloading, setDownloading] = useState(false);
```
3. After the existing `useEffect` (before `if (!settings)`), add:
```tsx
  // Know whether Stockfish works as soon as the screen opens, so a missing engine is obvious.
  useEffect(() => {
    api.checkEngine().then(setStatus, () => undefined);
  }, [api]);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let gone = false;
    api.onInstallProgress(setProgress).then((off) => {
      if (gone) off();
      else unsubscribe = off;
    });
    return () => {
      gone = true;
      unsubscribe?.();
    };
  }, [api]);
```
4. After the `check` function add:
```tsx
  // The label repeats the pinned version in crates/core/src/engine_install.rs.
  const download = async () => {
    setDownloading(true);
    setProgress(null);
    setMessage(null);
    try {
      const installed = await api.downloadStockfish();
      // The app saved the new path itself; show it without touching other unsaved edits.
      setSettings((current) => current && { ...current, engine_path: installed.path });
      setStatus({ found: true, name: installed.engine, error: null });
      setMessage({ text: `Installed ${installed.engine}.`, error: false });
    } catch (e) {
      setMessage({ text: errorMessage(e), error: true });
    } finally {
      setDownloading(false);
    }
  };
```
5. In the JSX, directly after the closing `</div>` of `settings__engine`, add:
```tsx
      {status && !status.found && (
        <div className="settings__download">
          <button type="button" onClick={download} disabled={downloading}>
            Download Stockfish 19
          </button>
          {downloading && (
            <>
              <progress value={installFraction(progress) ?? undefined} max={1} aria-label="Download progress" />
              <span role="status">{describeInstall(progress)}</span>
            </>
          )}
          <p className="muted">
            About 81 MB, from the official Stockfish release on GitHub. Stockfish is free software under the
            GNU GPL v3 and runs as a separate program.
          </p>
        </div>
      )}
```

- [ ] **Step 8: Add the styles**

Append to `app/src/styles.css` after the `.settings__field` block:

```css
.settings__download {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0 0 12px;
}

.settings__download progress {
  width: 100%;
  accent-color: var(--accent);
}
```

- [ ] **Step 9: Run the whole frontend suite and the type check**

Run: `cd app && npm test && npm run build`
Expected: all PASS; `tsc --noEmit` clean.

- [ ] **Step 10: Commit**

```bash
git add app/src
git commit -m "Download Stockfish from Settings" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: "Stockfish not found" banner on the home screen

**Files:**
- Modify: `app/src/screens/HomeScreen.tsx`, `app/src/screens/HomeScreen.test.tsx`, `app/src/App.tsx`, `app/src/App.test.tsx`, `app/src/styles.css`

**Interfaces:**
- Consumes: `Api.checkEngine()`.
- Produces: `HomeScreenProps.onOpenSettings?: () => void`.

- [ ] **Step 1: Write the failing tests**

In `app/src/screens/HomeScreen.test.tsx`, change `setup` to:

```tsx
function setup(options = {}) {
  const api = createFakeApi({ games: [operaGame, foolsMate], ...options });
  const onStart = vi.fn();
  const onOpen = vi.fn();
  const onOpenSettings = vi.fn();
  const view = render(
    <HomeScreen api={api} onStart={onStart} onOpen={onOpen} onOpenSettings={onOpenSettings} />,
  );
  return { api, onStart, onOpen, onOpenSettings, user: userEvent.setup(), ...view };
}
```

and add inside the `describe`:

```tsx
  it("says nothing about Stockfish when it works", async () => {
    const { api } = setup();
    await waitFor(() => expect(api.calls.some((c) => c[0] === "checkEngine")).toBe(true));
    expect(screen.queryByText(/Stockfish was not found/)).not.toBeInTheDocument();
  });

  it("tells the user when Stockfish is missing and leads them to Settings", async () => {
    const { user, onOpenSettings } = setup({
      engine: { found: false, name: null, error: "Stockfish was not found" },
    });
    expect(await screen.findByText(/Stockfish was not found, so games cannot be reviewed yet/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Get Stockfish in Settings" }));
    expect(onOpenSettings).toHaveBeenCalledOnce();
  });

  it("stays quiet when the engine check itself fails", async () => {
    const api = createFakeApi({ games: [operaGame] });
    api.checkEngine = async () => {
      throw "boom";
    };
    render(<HomeScreen api={api} onStart={vi.fn()} onOpen={vi.fn()} />);
    await screen.findByRole("heading", { name: "Review a game" });
    expect(screen.queryByText(/Stockfish was not found/)).not.toBeInTheDocument();
  });
```

In `app/src/App.test.tsx` add inside `describe("App", ...)`:

```tsx
  it("leads from the missing-Stockfish banner to Settings", async () => {
    const { user } = setup({ engine: { found: false, name: null, error: "Stockfish was not found" } });
    await user.click(await screen.findByRole("button", { name: "Get Stockfish in Settings" }));
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `cd app && npx vitest run src/screens/HomeScreen.test.tsx src/App.test.tsx`
Expected: FAIL (banner not rendered).

- [ ] **Step 3: Implement**

`app/src/screens/HomeScreen.tsx`: add `onOpenSettings?: () => void;` to `HomeScreenProps` (doc comment: "Opens Settings, where Stockfish can be downloaded."), destructure it in the component signature, add state and effect after `recent`:

```tsx
  const [engineMissing, setEngineMissing] = useState(false);
  useEffect(() => {
    api.checkEngine().then(
      (status) => setEngineMissing(!status.found),
      () => undefined,
    );
  }, [api]);
```

and in the JSX directly after `<h2>Review a game</h2>`:

```tsx
        {engineMissing && (
          <p className="home__notice" role="status">
            Stockfish was not found, so games cannot be reviewed yet.
            <button type="button" onClick={onOpenSettings}>
              Get Stockfish in Settings
            </button>
          </p>
        )}
```

`app/src/App.tsx`: pass `onOpenSettings={() => setView("settings")}` to `<HomeScreen ... />`.

`app/src/styles.css`: append after the Settings block:

```css
.home__notice {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0 0 12px;
  padding: 8px 12px;
  background: var(--panel-2);
  border: 1px solid var(--border);
  border-radius: 6px;
}
```

- [ ] **Step 4: Run the whole suite**

Run: `cd app && npm test && npm run build`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add app/src
git commit -m "Point to Settings when Stockfish is missing" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Version script

**Files:**
- Create: `scripts/version.mjs`, `scripts/version.test.mjs`
- Modify: `.github/workflows/ci.yml` (the `frontend` job)

**Interfaces:**
- Produces (used by Tasks 7-9): `node scripts/version.mjs check [vX.Y.Z]` (exit 1 on any disagreement, naming the file) and `node scripts/version.mjs set X.Y.Z`; exported `SPOTS`, `readVersions(root)`, `checkVersions(root, tag?) -> { expected, problems }`, `setVersion(root, version)`.

- [ ] **Step 1: Write the failing tests**

Create `scripts/version.test.mjs`:

```js
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
```

- [ ] **Step 2: Run to verify failure**

Run: `node --test scripts/version.test.mjs`
Expected: FAIL, "Cannot find module ... version.mjs".

- [ ] **Step 3: Implement**

Create `scripts/version.mjs`:

```js
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
```

- [ ] **Step 4: Run to verify it passes, then try the CLI**

```bash
node --test scripts/version.test.mjs
node scripts/version.mjs check
node scripts/version.mjs check v0.1.0
node scripts/version.mjs check v9.9.9 ; echo "exit $?"
```
Expected: tests PASS; the first two print "Every version field says 0.1.0."; the last prints eight problem lines and `exit 1`.

- [ ] **Step 5: Run it in CI**

In `.github/workflows/ci.yml`, at the end of the `frontend` job's steps add (the job's default working directory is `app`, so override it):

```yaml
      - name: Version fields agree, and the version script works
        working-directory: .
        run: |
          node scripts/version.mjs check
          node --test scripts/version.test.mjs
```

- [ ] **Step 6: Commit**

```bash
git add scripts/version.mjs scripts/version.test.mjs .github/workflows/ci.yml
git commit -m "Add a script that keeps the version in one state" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Turn on the Windows installer bundle

**Files:**
- Modify: `app/src-tauri/tauri.conf.json`

**Interfaces:**
- Produces (used by Task 7): `cargo`/`tauri build --bundles nsis` writes `target/release/bundle/nsis/Chess Analyzer_<version>_x64-setup.exe` (workspace `target/` at the repo root).

- [ ] **Step 1: Configure the bundle**

In `app/src-tauri/tauri.conf.json`, replace `"active": false,` in `bundle` with the lines below, keeping the `icon` list as it is:

```json
    "active": true,
    "targets": ["nsis"],
    "shortDescription": "Local chess game review powered by Stockfish",
    "windows": {
      "nsis": {
        "installMode": "currentUser"
      }
    },
```

- [ ] **Step 2: Build the installer locally**

```bash
cd app && npm ci && npm run tauri build -- --bundles nsis
```
Expected: ends with a line naming `target\release\bundle\nsis\Chess Analyzer_0.1.0_x64-setup.exe`. The first run compiles in release mode and downloads the NSIS tooling, which takes several minutes.

- [ ] **Step 3: Install it silently, run it, and remove it**

From the repository root (the workspace `target/` folder is there, not under `app/`):

```powershell
$installer = Get-ChildItem target\release\bundle\nsis\*-setup.exe | Select-Object -First 1
Start-Process -FilePath $installer.FullName -ArgumentList '/S' -Wait
Test-Path "$env:LOCALAPPDATA\Chess Analyzer\chess-analyzer-app.exe"
```
Expected: `True` with no administrator prompt (the exe name may differ; list `$env:LOCALAPPDATA\Chess Analyzer` if it prints `False`). Then launch it from the Start menu entry "Chess Analyzer" and confirm: the window opens, the **Stockfish-missing banner appears on the home screen** (if you have no `engines/` folder near the installed exe), and Settings shows the **Download Stockfish 19** button. Do not press it yet; Task 9 covers that on a clean profile. Uninstall with `& "$env:LOCALAPPDATA\Chess Analyzer\uninstall.exe" /S`.

- [ ] **Step 4: Commit**

```bash
git add app/src-tauri/tauri.conf.json
git commit -m "Build a per-user Windows installer" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Release workflow

**Files:**
- Modify: `.github/workflows/ci.yml` (add `workflow_call`)
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `scripts/version.mjs check <tag>` (Task 5); the installer path (Task 6).
- Produces: pushing a tag `vX.Y.Z[-pre]` creates a **draft** GitHub Release named "Chess Analyzer vX.Y.Z" with `…-setup.exe` and `…-setup.exe.sha256`, marked pre-release when the tag contains `-`.

- [ ] **Step 1: Let CI be called by other workflows**

In `.github/workflows/ci.yml` change the `on:` block to:

```yaml
on:
  push:
    branches: [master]
  pull_request:
  workflow_call:
```

- [ ] **Step 2: Create the release workflow**

Create `.github/workflows/release.yml`:

```yaml
name: Release

# Push a tag like v0.1.0 (or v0.1.0-rc1 for a pre-release) to build the installer and draft a
# GitHub Release. Read the draft, then publish it by hand. See docs/releasing.md.
on:
  push:
    tags: ["v*"]

permissions:
  contents: read

jobs:
  checks:
    uses: ./.github/workflows/ci.yml

  version:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: The tag matches the version in the files
        run: node scripts/version.mjs check "$GITHUB_REF_NAME"

  installer:
    needs: [checks, version]
    runs-on: windows-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
          cache-dependency-path: app/package-lock.json
      - name: Build the installer
        working-directory: app
        run: |
          npm ci
          npm run tauri build -- --bundles nsis
      - name: Checksum
        shell: pwsh
        run: |
          $installer = Get-ChildItem target/release/bundle/nsis/*-setup.exe | Select-Object -First 1
          $hash = (Get-FileHash $installer.FullName -Algorithm SHA256).Hash.ToLower()
          "$hash *$($installer.Name)" | Out-File -Encoding ascii "$($installer.FullName).sha256"
      - name: Draft the release
        shell: pwsh
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          $files = Get-ChildItem target/release/bundle/nsis/* -Include *-setup.exe, *.sha256
          $flags = @()
          if ($env:GITHUB_REF_NAME -like '*-*') { $flags += '--prerelease' }
          gh release create $env:GITHUB_REF_NAME --draft --generate-notes --title "Chess Analyzer $env:GITHUB_REF_NAME" @flags $files.FullName
```

- [ ] **Step 3: Check the YAML parses**

Run: `npx --yes js-yaml .github/workflows/release.yml > /dev/null && npx --yes js-yaml .github/workflows/ci.yml > /dev/null && echo ok`
Expected: `ok`. (The workflow itself can only be exercised by a real tag: Task 9.)

- [ ] **Step 4: Commit**

```bash
git add .github/workflows
git commit -m "Draft a GitHub Release when a version tag is pushed" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Documentation

**Files:**
- Modify: `README.md`, `AGENTS.md`
- Create: `docs/releasing.md`

- [ ] **Step 1: README install section**

In `README.md`, add a new section directly above `## Getting started`, titled `## Install (Windows)`:

```markdown
## Install (Windows)

Download `Chess Analyzer_<version>_x64-setup.exe` from the [latest release](https://github.com/KristijanJankuloski/chess-analyzer/releases/latest) and run it. It installs for your user only and does not ask for administrator rights.

Windows SmartScreen may say the app is from an unknown publisher: the installer is not code-signed yet. Choose **More info**, then **Run anyway**. The release page lists a SHA-256 checksum for the installer if you want to check the download.

The first time you open the app it tells you that Stockfish is missing. Open **Settings** and press **Download Stockfish 19** (about 81 MB, from the official Stockfish release on GitHub). That download is the only time the app uses the internet; analysis runs entirely on your machine. Already have Stockfish? Enter its path in Settings instead.

Building from source instead? Continue with "Getting started" below.
```

Also change the intro's last clause "with no account and no network" to "with no account, and no network once Stockfish is installed", so the README stays truthful about the one download.

- [ ] **Step 2: README licenses**

Replace the Licenses paragraph's second sentence ("Stockfish (GPLv3) is downloaded separately and run as its own process.") so it reads: "The installer does not contain Stockfish. Stockfish (GPLv3) is downloaded from its official release, either by the setup script or by the app's Download button, and run as its own process." Keep the opening-names sentence.

- [ ] **Step 3: Releasing guide**

Create `docs/releasing.md`:

````markdown
# Releasing

A release is a version tag. Pushing `vX.Y.Z` makes GitHub Actions build the Windows installer and draft a GitHub Release; you read the draft and publish it.

## Cut a release

1. Make sure `master` is green.
2. On a branch, set the version everywhere and review the diff:
   ```
   node scripts/version.mjs set 0.2.0
   git diff
   ```
   (It rewrites `Cargo.toml`, `app/src-tauri/tauri.conf.json`, `app/package.json`, `app/package-lock.json` and `Cargo.lock`.) Run `cargo build` once so nothing else in the lockfile moves, then commit as "Release 0.2.0".
3. Open a PR, merge it.
4. Tag the merge commit and push the tag:
   ```
   git checkout master && git pull
   git tag v0.2.0
   git push origin v0.2.0
   ```
5. Watch the **Release** run in GitHub Actions. It runs the full CI checks, refuses to continue if the tag and the files disagree, builds the installer and creates a **draft** release with the installer and its `.sha256`.
6. Open the draft, edit the generated notes, and publish.

## Try a release first

Use a pre-release tag such as `v0.2.0-rc1` (set the version to `0.2.0-rc1` first). Any tag containing `-` is published as a pre-release. Install it on a machine or Windows profile that has never had the app, download Stockfish from Settings, and review a game. Then set the version to the real one and tag again.

## When it goes wrong

- *"... says 0.1.0, expected 0.2.0"*: the files and the tag disagree. Delete the tag (`git push origin :refs/tags/v0.2.0` and `git tag -d v0.2.0`), fix the version, tag again.
- *Installer built but the draft was not created*: re-run the failed job; the build is repeatable. If a draft exists for that tag, delete the draft first.
- *Windows SmartScreen warning*: expected until the installer is code-signed (not done yet).

## Updating the bundled Stockfish version

The in-app download is pinned in `crates/core/src/engine_install.rs` (tag, asset name and SHA-256). To move to a newer Stockfish, change those three constants, then run `cargo test -p chess-analyzer-core --test stockfish installs_the_real_release -- --ignored` to prove the new release downloads, verifies and starts. Update the button label in `SettingsScreen.tsx` and the setup scripts' default version to match.
````

- [ ] **Step 4: AGENTS.md**

In `AGENTS.md` under "Open Decisions", replace the line "- How Stockfish and model binaries are distributed (bundled vs. downloaded on first run)." with:

```markdown
- How model binaries are distributed. (Stockfish is decided: the Windows installer does not bundle it; the app offers a one-click download of the pinned official release, verified by SHA-256. See `docs/superpowers/specs/2026-10-09-windows-release-design.md`.)
```

- [ ] **Step 5: Commit**

```bash
git add README.md AGENTS.md docs/releasing.md
git commit -m "Document installing and releasing" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Dry run with a pre-release (manual; ask before each outward step)

Pushing tags and branches publishes things, so **stop and get the maintainer's go-ahead before each push**. Nothing in this task is automated.

- [ ] **Step 1: Full local verification before pushing**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo clippy -p chess-analyzer-app --all-targets -- -D warnings
node scripts/version.mjs check
node --test scripts/version.test.mjs
cd app && npm test && npm run build
```
Expected: everything passes; `git status` clean (the generated TypeScript is committed).

- [ ] **Step 2: Open the PR and get CI green**

With approval: `git push -u origin feature/windows-release` and open a PR "Windows installer and in-app Stockfish download". Wait for CI to pass on Ubuntu and Windows.

- [ ] **Step 3: Release candidate from the branch**

With approval:
```bash
node scripts/version.mjs set 0.1.0-rc1
git commit -am "Release 0.1.0-rc1" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
git tag v0.1.0-rc1
git push origin feature/windows-release v0.1.0-rc1
```
Expected: the **Release** workflow runs (checks, version, installer) and leaves a **draft pre-release** "Chess Analyzer v0.1.0-rc1" with `Chess Analyzer_0.1.0-rc1_x64-setup.exe` and `.sha256`. If the version job fails, fix as `docs/releasing.md` says.

- [ ] **Step 4: Try the installer on a clean profile**

Download the draft's installer (the draft is visible to repository writers). Check `Get-FileHash` against the `.sha256`. On a Windows account that has never run the app: install (no admin prompt), launch, confirm the banner, press **Download Stockfish 19** in Settings, watch the progress bar, confirm "Found Stockfish 19", paste a PGN and review it. Then test: (a) turn off the network and press Download on a fresh profile: you should see a plain error and be able to retry; (b) install the same installer again over the top: settings and games survive.

- [ ] **Step 5: Real release**

If the rc passes, with approval: delete the draft and tag (`gh release delete v0.1.0-rc1 --yes --cleanup-tag`), then `node scripts/version.mjs set 0.1.0`, commit "Release 0.1.0", merge the PR, tag `v0.1.0` on master, push the tag, read the draft, and publish.

---

## Self-review notes

- **Spec coverage:** Section 1 (download, hash, handshake, settings path, command, UI, failure cases) is Tasks 1-4; Section 2 (NSIS bundle, version script + CI check, tag-driven workflow, draft/pre-release, README, license wording) is Tasks 5-8; Section 3 (unit, integration, frontend, version, manual rc) is spread across Tasks 1-5 and 9. Deviations are listed at the top.
- **Types:** `InstallProgress` (`stage`: `downloading` / `verifying` / `installing`), `Installed { path, engine }`, `install_stockfish`, `InstallLock::try_acquire`, `downloadStockfish`, `onInstallProgress`, `emitInstall`, `describeInstall`, `installFraction`, `checkVersions`/`setVersion`/`readVersions` are spelled the same wherever they are used.
