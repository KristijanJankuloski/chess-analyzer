//! The desktop shell: a thin layer that forwards frontend commands to `chess-analyzer-core`
//! and core's job events back to the frontend. All behaviour worth testing lives in core.

use std::path::Path;
use std::sync::{Arc, Mutex};

use chess_analyzer_core::cache::{CachedAnalyzer, open_database};
use chess_analyzer_core::engine::{Analyzer, EngineError};
use chess_analyzer_core::game::{
    MAX_PGN_BYTES, PgnGameInfo, describe_pgn, read_pgn_file as read_pgn_file_limited,
};
use chess_analyzer_core::jobs::{
    EngineFactory, EventSink, JobEvent, JobId, ReviewJobs, ReviewSource, StartedJob,
};
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::settings::{EngineStatus, Settings, SettingsFile, check_engine};
use chess_analyzer_core::store::{GameStore, GameSummary, StoredGame};
use tauri::{Emitter, Manager, State};

/// The event name the frontend listens on.
const REVIEW_EVENT: &str = "review-event";

struct TauriSink(tauri::AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: JobEvent) {
        if let Err(e) = self.0.emit(REVIEW_EVENT, &event) {
            eprintln!("could not send {REVIEW_EVENT}: {e}");
        }
    }
}

struct AppState {
    jobs: ReviewJobs,
    store: Arc<Mutex<GameStore>>,
    settings: Mutex<Settings>,
    settings_file: SettingsFile,
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

/// Starting the engine takes a moment, so it runs off the main thread.
#[tauri::command]
async fn check_engine_status(state: State<'_, AppState>) -> Result<EngineStatus, String> {
    let settings = state.settings.lock().map_err(message)?.clone();
    tauri::async_runtime::spawn_blocking(move || check_engine(&settings))
        .await
        .map_err(message)
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Chess Analyzer app");
}
