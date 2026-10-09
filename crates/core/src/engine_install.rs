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
    format!(
        "https://github.com/official-stockfish/Stockfish/releases/download/{STOCKFISH_TAG}/{ASSET_NAME}"
    )
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
    #[error(
        "the download is not the expected Stockfish release (checksum mismatch), so it was discarded"
    )]
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
        if entry.is_file()
            && file_name.starts_with("stockfish")
            && file_name.ends_with(".exe")
            && !in_sources
        {
            io::copy(&mut entry, &mut File::create(target)?)?;
            return Ok(());
        }
    }
    Err(InstallError::Archive(
        "no Stockfish program in the download".into(),
    ))
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
        zip_with(&[
            ("stockfish/Copying.txt", b"GPL"),
            (ENGINE_ENTRY, b"engine bytes"),
        ])
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
        install_from(Cursor::new(body), total, sha, dir, verify, &mut |p| {
            seen.push(p)
        })
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
        assert!(
            ASSET_SHA256
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
    }

    #[test]
    fn the_checksum_is_computed_while_copying() {
        let mut out = Vec::new();
        let digest = copy_hashed(
            &mut Cursor::new(b"abc".to_vec()),
            &mut out,
            Some(3),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
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
        assert!(
            sizes.len() < 20,
            "reported in chunks, not on every read: {}",
            sizes.len()
        );
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
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            2,
            "only the archive and the engine"
        );
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
        let err = install(
            body.clone(),
            &sha256_hex(&body),
            &dir,
            &ok_engine,
            &mut Vec::new(),
        )
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
        assert_eq!(
            stages,
            [&InstallProgress::Verifying, &InstallProgress::Installing]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_checksum_mismatch_installs_nothing() {
        let dir = temp_dir("mismatch");
        let err = install(
            engine_zip(),
            &"0".repeat(64),
            &dir,
            &ok_engine,
            &mut Vec::new(),
        )
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
            Err(InstallError::Engine(EngineError::Protocol(
                "no uciok".into(),
            )))
        };
        let err = install(
            zip.clone(),
            &sha256_hex(&zip),
            &dir,
            &broken,
            &mut Vec::new(),
        )
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
        install(
            zip.clone(),
            &sha256_hex(&zip),
            &dir,
            &ok_engine,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(fs::read(dir.join(ENGINE_FILE)).unwrap(), b"engine bytes");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_leftover_scratch_folder_does_not_get_in_the_way() {
        let dir = temp_dir("leftover");
        fs::create_dir_all(dir.join(WORK_DIR)).unwrap();
        fs::write(dir.join(WORK_DIR).join("junk"), b"from a crashed run").unwrap();
        let zip = engine_zip();
        install(
            zip.clone(),
            &sha256_hex(&zip),
            &dir,
            &ok_engine,
            &mut Vec::new(),
        )
        .unwrap();
        assert!(!dir.join(WORK_DIR).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    struct DropsAfter(usize);

    impl Read for DropsAfter {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "connection reset",
                ));
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
        assert!(
            matches!(&err, InstallError::Download(m) if m.contains("connection reset")),
            "{err}"
        );
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
