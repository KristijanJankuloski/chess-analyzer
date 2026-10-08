//! User settings, persisted as a small JSON file, and the engine described by them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use crate::engine::{Analyzer, EngineConfig, EngineError, Limits, UciEngine, locate_stockfish};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("invalid settings: {0}")]
    Invalid(String),
    #[error("could not read or write the settings file: {0}")]
    Io(String),
    #[error("the settings file is not valid JSON: {0}")]
    Corrupt(String),
}

/// Upper limits for the numeric settings. They keep a typo (or a hand-edited file) from asking
/// Stockfish for a search that can never finish or for more memory than any machine has. The
/// Settings screen in `app/src/screens/SettingsScreen.tsx` repeats them as `max` attributes.
pub const MAX_DEPTH: u32 = 60;
pub const MAX_MULTIPV: u32 = 10;
pub const MAX_THREADS: u32 = 256;
pub const MAX_HASH_MB: u32 = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(default)]
pub struct Settings {
    /// Path to a Stockfish executable; `None` means look in the usual places.
    pub engine_path: Option<String>,
    pub threads: u32,
    /// Stockfish hash size in MB.
    pub hash_mb: u32,
    pub depth: u32,
    pub multipv: u32,
}

impl Default for Settings {
    fn default() -> Settings {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        Settings {
            engine_path: None,
            threads: (cores / 2).clamp(1, 8) as u32,
            hash_mb: 256,
            depth: 20,
            multipv: 3,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        for (name, value, max) in [
            ("threads", self.threads, MAX_THREADS),
            ("hash_mb", self.hash_mb, MAX_HASH_MB),
            ("depth", self.depth, MAX_DEPTH),
            ("multipv", self.multipv, MAX_MULTIPV),
        ] {
            if !(1..=max).contains(&value) {
                return Err(SettingsError::Invalid(format!(
                    "{name} must be between 1 and {max}"
                )));
            }
        }
        Ok(())
    }

    pub fn limits(&self) -> Limits {
        Limits {
            depth: self.depth,
            multipv: self.multipv,
        }
    }

    /// Starts the engine these settings describe.
    pub fn start_engine(&self) -> Result<UciEngine, EngineError> {
        let explicit = self.engine_path.as_deref().map(Path::new);
        let path = locate_stockfish(explicit).ok_or(EngineError::NotFound)?;
        let mut config = EngineConfig::new(path);
        config.threads = self.threads;
        config.hash_mb = self.hash_mb;
        UciEngine::start(config)
    }
}

/// What the Settings screen shows about the configured engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EngineStatus {
    pub found: bool,
    /// The engine's own name and version, e.g. "Stockfish 19".
    pub name: Option<String>,
    /// Why it could not be used, when `found` is false.
    pub error: Option<String>,
}

/// Starts the configured engine briefly to see whether it works.
pub fn check_engine(settings: &Settings) -> EngineStatus {
    match settings.start_engine() {
        Ok(engine) => EngineStatus {
            found: true,
            name: Some(engine.engine_id()),
            error: None,
        },
        Err(e) => EngineStatus {
            found: false,
            name: None,
            error: Some(e.to_string()),
        },
    }
}

/// The settings file on disk.
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn new(path: PathBuf) -> SettingsFile {
        SettingsFile { path }
    }

    /// A missing file means defaults; a file that cannot be read or parsed is an error.
    pub fn load(&self) -> Result<Settings, SettingsError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(SettingsError::Io(e.to_string())),
        };
        let settings: Settings =
            serde_json::from_str(&text).map_err(|e| SettingsError::Corrupt(e.to_string()))?;
        settings.validate()?;
        Ok(settings)
    }

    /// Validates, then writes through a temporary file so a crash cannot leave half a file.
    pub fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        settings.validate()?;
        let json =
            serde_json::to_string_pretty(settings).map_err(|e| SettingsError::Io(e.to_string()))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| SettingsError::Io(e.to_string()))?;
        }
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, json).map_err(|e| SettingsError::Io(e.to_string()))?;
        std::fs::rename(&temp, &self.path).map_err(|e| SettingsError::Io(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> (PathBuf, SettingsFile) {
        let dir = std::env::temp_dir().join(format!(
            "chess-analyzer-settings-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let file = SettingsFile::new(dir.join("nested").join("settings.json"));
        (dir, file)
    }

    #[test]
    fn defaults_are_valid_and_sensible() {
        let s = Settings::default();
        assert!(s.validate().is_ok());
        assert!(s.threads >= 1);
        assert_eq!((s.depth, s.multipv, s.hash_mb), (20, 3, 256));
        assert_eq!(s.engine_path, None);
        assert_eq!(
            s.limits(),
            Limits {
                depth: 20,
                multipv: 3
            }
        );
    }

    #[test]
    fn zero_values_are_invalid() {
        for field in ["threads", "hash_mb", "depth", "multipv"] {
            let mut s = Settings::default();
            match field {
                "threads" => s.threads = 0,
                "hash_mb" => s.hash_mb = 0,
                "depth" => s.depth = 0,
                _ => s.multipv = 0,
            }
            let err = s.validate().unwrap_err();
            assert!(err.to_string().contains(field), "{err}");
        }
    }

    #[test]
    fn values_above_the_limits_are_invalid() {
        let over = [
            (
                "depth",
                Settings {
                    depth: MAX_DEPTH + 1,
                    ..Settings::default()
                },
            ),
            (
                "multipv",
                Settings {
                    multipv: MAX_MULTIPV + 1,
                    ..Settings::default()
                },
            ),
            (
                "threads",
                Settings {
                    threads: MAX_THREADS + 1,
                    ..Settings::default()
                },
            ),
            (
                "hash_mb",
                Settings {
                    hash_mb: MAX_HASH_MB + 1,
                    ..Settings::default()
                },
            ),
        ];
        for (field, settings) in over {
            let err = settings.validate().unwrap_err();
            assert!(matches!(err, SettingsError::Invalid(_)), "{field}: {err}");
            assert!(err.to_string().contains(field), "{err}");
        }
    }

    #[test]
    fn the_limits_themselves_are_allowed() {
        let at_the_limits = Settings {
            engine_path: None,
            threads: MAX_THREADS,
            hash_mb: MAX_HASH_MB,
            depth: MAX_DEPTH,
            multipv: MAX_MULTIPV,
        };
        assert!(at_the_limits.validate().is_ok());
    }

    #[test]
    fn a_settings_file_with_an_absurd_value_is_refused_not_trusted() {
        let (dir, file) = temp_file("absurd");
        std::fs::create_dir_all(file.path.parent().unwrap()).unwrap();
        std::fs::write(&file.path, r#"{"depth": 5000}"#).unwrap();
        assert!(matches!(file.load(), Err(SettingsError::Invalid(_))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_file_loads_the_defaults() {
        let (dir, file) = temp_file("missing");
        assert_eq!(file.load().unwrap(), Settings::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_round_trip_through_the_file_and_create_the_directory() {
        let (dir, file) = temp_file("roundtrip");
        let settings = Settings {
            engine_path: Some("C:/engines/stockfish.exe".into()),
            threads: 4,
            hash_mb: 1024,
            depth: 14,
            multipv: 2,
        };
        file.save(&settings).unwrap();
        assert_eq!(file.load().unwrap(), settings);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_or_missing_fields_fall_back_to_defaults() {
        let (dir, file) = temp_file("partial");
        std::fs::create_dir_all(file.path.parent().unwrap()).unwrap();
        std::fs::write(&file.path, r#"{"depth": 12, "from_a_newer_version": true}"#).unwrap();
        let loaded = file.load().unwrap();
        assert_eq!(loaded.depth, 12);
        assert_eq!(loaded.multipv, 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_silently_replaced() {
        let (dir, file) = temp_file("corrupt");
        std::fs::create_dir_all(file.path.parent().unwrap()).unwrap();
        std::fs::write(&file.path, "{ not json").unwrap();
        assert!(matches!(file.load(), Err(SettingsError::Corrupt(_))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_settings_are_not_saved() {
        let (dir, file) = temp_file("invalid");
        let settings = Settings {
            depth: 0,
            ..Settings::default()
        };
        assert!(matches!(
            file.save(&settings),
            Err(SettingsError::Invalid(_))
        ));
        assert!(!file.path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_explicit_engine_is_reported_not_papered_over() {
        let settings = Settings {
            engine_path: Some("definitely/not/here/stockfish".into()),
            ..Settings::default()
        };
        let status = check_engine(&settings);
        assert!(!status.found);
        assert!(status.error.unwrap().contains("Stockfish was not found"));
    }
}
