//! The desktop shell: a thin layer that forwards frontend commands to `chess-analyzer-core`
//! and core's job events back to the frontend. All behaviour worth testing lives in core.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chess_analyzer_core::cache::{CachedAnalyzer, open_database};
use chess_analyzer_core::classify::Thresholds;
use chess_analyzer_core::engine::{Analyzer, EngineError};
use chess_analyzer_core::engine_install::{InstallLock, Installed, install_stockfish};
use chess_analyzer_core::game::{
    MAX_PGN_BYTES, PgnGameInfo, describe_pgn, read_pgn_file as read_pgn_file_limited,
};
use chess_analyzer_core::jobs::{
    EngineFactory, EventSink, JobEvent, JobId, ReviewJobs, ReviewSource, StartedJob,
};
use chess_analyzer_core::live::{LiveConfig, LiveEvent, LiveSession, LiveSink};
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::settings::{EngineStatus, Settings, SettingsFile, check_engine};
use chess_analyzer_core::store::{GameStore, GameSummary, StoredGame};
use tauri::{Emitter, Manager, State};

/// The event names the frontend listens on.
const REVIEW_EVENT: &str = "review-event";
const LIVE_EVENT: &str = "live-event";
const INSTALL_EVENT: &str = "install-progress";

struct TauriSink(tauri::AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: JobEvent) {
        if let Err(e) = self.0.emit(REVIEW_EVENT, &event) {
            eprintln!("could not send {REVIEW_EVENT}: {e}");
        }
    }
}

struct TauriLiveSink(tauri::AppHandle);

impl LiveSink for TauriLiveSink {
    fn emit(&self, event: LiveEvent) {
        if let Err(e) = self.0.emit(LIVE_EVENT, &event) {
            eprintln!("could not send {LIVE_EVENT}: {e}");
        }
    }
}

/// The settings a live session was built from. Changing any of them means a new engine.
#[derive(PartialEq)]
struct EngineKey {
    engine_path: Option<String>,
    threads: u32,
    hash_mb: u32,
    multipv: u32,
}

impl EngineKey {
    fn of(settings: &Settings) -> EngineKey {
        EngineKey {
            engine_path: settings.engine_path.clone(),
            threads: settings.threads,
            hash_mb: settings.hash_mb,
            multipv: settings.multipv,
        }
    }
}

struct RunningLive {
    session: LiveSession,
    key: EngineKey,
}

struct AppState {
    jobs: ReviewJobs,
    store: Arc<Mutex<GameStore>>,
    settings: Mutex<Settings>,
    settings_file: SettingsFile,
    handle: tauri::AppHandle,
    live: Mutex<Option<RunningLive>>,
    /// Where a downloaded Stockfish is kept (inside the app's data directory).
    engines_dir: PathBuf,
    install_lock: InstallLock,
}

fn message(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command(async)]
fn parse_pgn_games(text: String) -> Result<Vec<PgnGameInfo>, String> {
    describe_pgn(&text).map_err(message)
}

/// Plain `#[tauri::command]`s run on the main thread, which would freeze the window while a
/// big PGN is read or parsed; `(async)` runs them on a worker thread instead.
#[tauri::command(async)]
fn read_pgn_file(path: String) -> Result<String, String> {
    read_pgn_file_limited(Path::new(&path), MAX_PGN_BYTES).map_err(message)
}

#[tauri::command(async)]
fn start_review(state: State<'_, AppState>, source: ReviewSource) -> Result<StartedJob, String> {
    let settings = state.settings.lock().map_err(message)?.clone();
    state.jobs.start(source, settings).map_err(message)
}

#[tauri::command]
fn cancel_review(state: State<'_, AppState>, job: JobId) -> bool {
    state.jobs.cancel(job)
}

#[tauri::command]
fn list_games(state: State<'_, AppState>) -> Result<Vec<GameSummary>, String> {
    state.store.lock().map_err(message)?.list().map_err(message)
}

#[tauri::command]
fn get_game(state: State<'_, AppState>, id: i64) -> Result<Option<StoredGame>, String> {
    state
        .store
        .lock()
        .map_err(message)?
        .get(id)
        .map_err(message)
}

#[tauri::command]
fn delete_game(state: State<'_, AppState>, id: i64) -> Result<bool, String> {
    state
        .store
        .lock()
        .map_err(message)?
        .delete(id)
        .map_err(message)
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.lock().map_err(message)?.clone())
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    state.settings_file.save(&settings).map_err(message)?;
    *state.settings.lock().map_err(message)? = settings.clone();
    Ok(settings)
}

/// Tells the live analysis the complete list of moves (UCI) so far, starting it on first use.
/// Starting the engine takes a moment, so this runs off the main thread.
#[tauri::command(async)]
fn live_update(
    state: State<'_, AppState>,
    revision: u64,
    moves: Vec<String>,
) -> Result<(), String> {
    let settings = state.settings.lock().map_err(message)?.clone();
    settings.validate().map_err(message)?;
    let key = EngineKey::of(&settings);
    let mut live = state.live.lock().map_err(message)?;
    if live
        .as_ref()
        .is_none_or(|running| running.key != key || !running.session.is_running())
    {
        // Stop the old engine before the next one starts (a session whose worker died after an
        // internal error is replaced too, so that asking again works).
        *live = None;
        let engine = settings.start_engine().map_err(message)?;
        let config = LiveConfig {
            multipv: settings.multipv,
            thresholds: Thresholds::default(),
            book: OpeningBook::bundled(),
        };
        let sink = Arc::new(TauriLiveSink(state.handle.clone()));
        *live = Some(RunningLive {
            session: LiveSession::start(engine, sink, config),
            key,
        });
    }
    if let Some(running) = live.as_ref() {
        running.session.set_moves(revision, moves);
    }
    Ok(())
}

/// Stops searching while keeping the analyses (the user went to another screen). It runs off
/// the main thread because `live_update` can hold the session while an engine starts or stops,
/// and the window must not wait for that.
#[tauri::command(async)]
fn live_pause(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(running) = state.live.lock().map_err(message)?.as_ref() {
        running.session.pause();
    }
    Ok(())
}

/// Starting the engine takes a moment, so it runs off the main thread.
#[tauri::command]
async fn check_engine_status(state: State<'_, AppState>) -> Result<EngineStatus, String> {
    let settings = state.settings.lock().map_err(message)?.clone();
    tauri::async_runtime::spawn_blocking(move || check_engine(&settings))
        .await
        .map_err(message)
}

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

fn engine_factory(cache_path: std::path::PathBuf) -> Arc<EngineFactory> {
    Arc::new(
        move |settings: &Settings| -> Result<Box<dyn Analyzer + Send>, EngineError> {
            let engine = settings.start_engine()?;
            Ok(match open_database(&cache_path) {
                Ok(conn) => Box::new(CachedAnalyzer::new(engine, conn)),
                Err(e) => {
                    eprintln!("{e}; reviewing without the analysis cache");
                    Box::new(engine)
                }
            })
        },
    )
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;

            let settings_file = SettingsFile::new(dir.join("settings.json"));
            let settings = settings_file.load().unwrap_or_else(|e| {
                eprintln!("{e}; using default settings");
                Settings::default()
            });
            let store = Arc::new(Mutex::new(GameStore::open(&dir.join("games.db"))?));
            let jobs = ReviewJobs::new(
                engine_factory(dir.join("analysis-cache.db")),
                Arc::clone(&store),
                Arc::new(TauriSink(app.handle().clone())),
                OpeningBook::bundled(),
            );
            app.manage(AppState {
                jobs,
                store,
                settings: Mutex::new(settings),
                settings_file,
                handle: app.handle().clone(),
                live: Mutex::new(None),
                engines_dir: dir.join("engines"),
                install_lock: InstallLock::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            parse_pgn_games,
            read_pgn_file,
            start_review,
            cancel_review,
            list_games,
            get_game,
            delete_game,
            get_settings,
            save_settings,
            check_engine_status,
            download_stockfish,
            live_update,
            live_pause,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Chess Analyzer app");
}
