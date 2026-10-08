//! SQLite cache of engine analyses, as a decorator around any `Analyzer`.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::engine::{Analyzer, EngineError, Limits, PositionAnalysis};

#[derive(Debug, Error)]
#[error("analysis cache unavailable: {0}")]
pub struct CacheError(String);

impl From<rusqlite::Error> for CacheError {
    fn from(e: rusqlite::Error) -> CacheError {
        CacheError(e.to_string())
    }
}

/// Looks positions up by (FEN, engine id, depth, MultiPV) before asking the wrapped analyzer.
pub struct CachedAnalyzer<A: Analyzer> {
    inner: A,
    conn: Connection,
    pub hits: usize,
    pub misses: usize,
}

/// Opens (creating it if needed) a cache database. Fails if the file cannot be opened,
/// is not a database, or cannot be written. Callers decide whether to carry on uncached.
pub fn open_database(path: &Path) -> Result<Connection, CacheError> {
    prepare(Connection::open(path)?)
}

pub fn in_memory_database() -> Result<Connection, CacheError> {
    prepare(Connection::open_in_memory()?)
}

fn prepare(conn: Connection) -> Result<Connection, CacheError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS analysis (
            fen      TEXT    NOT NULL,
            engine   TEXT    NOT NULL,
            depth    INTEGER NOT NULL,
            multipv  INTEGER NOT NULL,
            result   TEXT    NOT NULL,
            PRIMARY KEY (fen, engine, depth, multipv)
        )",
    )?;
    Ok(conn)
}

impl<A: Analyzer> CachedAnalyzer<A> {
    pub fn new(inner: A, conn: Connection) -> CachedAnalyzer<A> {
        CachedAnalyzer {
            inner,
            conn,
            hits: 0,
            misses: 0,
        }
    }

    pub fn open(inner: A, path: &Path) -> Result<CachedAnalyzer<A>, CacheError> {
        Ok(CachedAnalyzer::new(inner, open_database(path)?))
    }

    pub fn in_memory(inner: A) -> Result<CachedAnalyzer<A>, CacheError> {
        Ok(CachedAnalyzer::new(inner, in_memory_database()?))
    }

    fn lookup(&self, fen: &str, engine: &str, limits: &Limits) -> Option<PositionAnalysis> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT result FROM analysis WHERE fen = ?1 AND engine = ?2 AND depth = ?3 AND multipv = ?4",
                params![fen, engine, limits.depth, limits.multipv],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        json.and_then(|j| serde_json::from_str(&j).ok())
    }

    fn store(&self, fen: &str, engine: &str, limits: &Limits, analysis: &PositionAnalysis) {
        // A failed write only costs a future cache hit, so it never fails the review.
        if let Ok(json) = serde_json::to_string(analysis) {
            let _ = self.conn.execute(
                "INSERT OR REPLACE INTO analysis (fen, engine, depth, multipv, result) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![fen, engine, limits.depth, limits.multipv, json],
            );
        }
    }
}

/// The cache key for a position: board, side to move, castling and en passant. The move/// counters are dropped so the same position reached by a different move order shares an entry.fn position_key(fen: &str) -> String {    fen.split(' ').take(4).collect::<Vec<_>>().join(" ")}
/// The cache key for a position: board, side to move, castling and en passant. The move
/// counters are dropped so the same position reached by a different move order shares an entry.
fn position_key(fen: &str) -> String {
    fen.split(' ').take(4).collect::<Vec<_>>().join(" ")
}

impl<A: Analyzer> Analyzer for CachedAnalyzer<A> {
    fn analyze(&mut self, fen: &str, limits: &Limits) -> Result<PositionAnalysis, EngineError> {
        let engine = self.inner.engine_id();
        let key = position_key(fen);
        if let Some(hit) = self.lookup(&key, &engine, limits) {
            self.hits += 1;
            return Ok(hit);
        }
        self.misses += 1;
        let analysis = self.inner.analyze(fen, limits)?;
        self.store(&key, &engine, limits, &analysis);
        Ok(analysis)
    }

    fn engine_id(&self) -> String {
        self.inner.engine_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AnalysisLine, ScriptedAnalyzer};
    use crate::eval::Eval;

    fn analysis(cp: i32) -> PositionAnalysis {
        PositionAnalysis {
            lines: vec![AnalysisLine {
                rank: 1,
                eval: Eval::Cp(cp),
                depth: 20,
                pv: vec!["e2e4".into()],
            }],
        }
    }

    #[test]
    fn second_lookup_is_served_from_the_cache() {
        let scripted = ScriptedAnalyzer::new(vec![analysis(10)]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        let limits = Limits::default();
        let first = cached.analyze("fen-a", &limits).unwrap();
        let second = cached.analyze("fen-a", &limits).unwrap();
        assert_eq!(first, second);
        assert_eq!((cached.hits, cached.misses), (1, 1));
    }

    #[test]
    fn different_limits_or_positions_miss() {
        let scripted = ScriptedAnalyzer::new(vec![analysis(1), analysis(2), analysis(3)]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        cached
            .analyze(
                "fen-a",
                &Limits {
                    depth: 10,
                    multipv: 1,
                },
            )
            .unwrap();
        cached
            .analyze(
                "fen-a",
                &Limits {
                    depth: 12,
                    multipv: 1,
                },
            )
            .unwrap();
        cached
            .analyze(
                "fen-b",
                &Limits {
                    depth: 10,
                    multipv: 1,
                },
            )
            .unwrap();
        assert_eq!((cached.hits, cached.misses), (0, 3));
    }

    #[test]
    fn positions_that_differ_only_in_move_counters_share_an_entry() {
        let scripted = ScriptedAnalyzer::new(vec![analysis(5)]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        let limits = Limits::default();
        let a = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1";
        let b = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 7 12";
        cached.analyze(a, &limits).unwrap();
        cached.analyze(b, &limits).unwrap();
        assert_eq!((cached.hits, cached.misses), (1, 1));
    }

    #[test]
    fn a_different_side_to_move_is_a_different_position() {
        let scripted = ScriptedAnalyzer::new(vec![analysis(5), analysis(6)]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        let limits = Limits::default();
        cached
            .analyze("8/8/8/8/8/8/8/K1k5 w - - 0 1", &limits)
            .unwrap();
        cached
            .analyze("8/8/8/8/8/8/8/K1k5 b - - 0 1", &limits)
            .unwrap();
        assert_eq!((cached.hits, cached.misses), (0, 2));
    }
    #[test]
    fn engine_errors_are_not_cached() {
        let scripted = ScriptedAnalyzer::new(vec![]);
        let mut cached = CachedAnalyzer::in_memory(scripted).unwrap();
        assert!(cached.analyze("fen-a", &Limits::default()).is_err());
        assert_eq!(cached.hits, 0);
    }

    #[test]
    fn the_cache_survives_reopening_the_file() {
        let dir =
            std::env::temp_dir().join(format!("chess-analyzer-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.db");
        let _ = std::fs::remove_file(&path);

        let mut first =
            CachedAnalyzer::open(ScriptedAnalyzer::new(vec![analysis(7)]), &path).unwrap();
        first.analyze("fen-a", &Limits::default()).unwrap();
        drop(first);

        let mut second = CachedAnalyzer::open(ScriptedAnalyzer::new(vec![]), &path).unwrap();
        let hit = second.analyze("fen-a", &Limits::default()).unwrap();
        assert_eq!(hit, analysis(7));
        assert_eq!((second.hits, second.misses), (1, 0));
        drop(second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_is_reported() {
        let dir = std::env::temp_dir().join(format!(
            "chess-analyzer-corrupt-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.db");
        std::fs::write(
            &path,
            b"this is not a sqlite database, just some text padding it out to be long enough",
        )
        .unwrap();
        assert!(open_database(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unwritable_path_is_reported() {
        let result = CachedAnalyzer::open(
            ScriptedAnalyzer::new(vec![]),
            Path::new("definitely/not/a/real/dir/cache.db"),
        );
        assert!(result.is_err());
    }
}
