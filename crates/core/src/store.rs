//! Finished reviews, kept in SQLite so past games can be reopened without re-analysing.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use crate::game::Game;
use crate::review::{Accuracy, Review, backfill_commentary};

#[derive(Debug, Error)]
#[error("game store error: {0}")]
pub struct StoreError(String);

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> StoreError {
        StoreError(e.to_string())
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> StoreError {
        StoreError(e.to_string())
    }
}

/// One row of the "recent games" list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GameSummary {
    #[ts(type = "number")]
    pub id: i64,
    /// Seconds since the Unix epoch.
    #[ts(type = "number")]
    pub created_at: i64,
    pub white: String,
    pub black: String,
    pub result: String,
    pub opening: Option<String>,
    /// Number of half-moves.
    pub moves: u32,
    pub accuracy: Accuracy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StoredGame {
    pub summary: GameSummary,
    pub game: Game,
    pub review: Review,
}

pub struct GameStore {
    conn: Connection,
}

const SUMMARY_COLUMNS: &str =
    "id, created_at, white, black, result, opening, moves, accuracy_white, accuracy_black";

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<GameSummary> {
    Ok(GameSummary {
        id: row.get(0)?,
        created_at: row.get(1)?,
        white: row.get(2)?,
        black: row.get(3)?,
        result: row.get(4)?,
        opening: row.get(5)?,
        moves: row.get(6)?,
        accuracy: Accuracy {
            white: row.get(7)?,
            black: row.get(8)?,
        },
    })
}

impl GameStore {
    pub fn open(path: &Path) -> Result<GameStore, StoreError> {
        GameStore::with_connection(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<GameStore, StoreError> {
        GameStore::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(conn: Connection) -> Result<GameStore, StoreError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS games (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at     INTEGER NOT NULL,
                white          TEXT    NOT NULL,
                black          TEXT    NOT NULL,
                result         TEXT    NOT NULL,
                opening        TEXT,
                moves          INTEGER NOT NULL,
                accuracy_white REAL,
                accuracy_black REAL,
                game_json      TEXT    NOT NULL,
                review_json    TEXT    NOT NULL
            )",
        )?;
        Ok(GameStore { conn })
    }

    /// Saves a finished review and returns the new game id.
    pub fn save(&self, game: &Game, review: &Review) -> Result<i64, StoreError> {
        let header = |key: &str| game.headers.get(key).cloned().unwrap_or_else(|| "?".into());
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        self.conn.execute(
            "INSERT INTO games (created_at, white, black, result, opening, moves,
                                accuracy_white, accuracy_black, game_json, review_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                created_at,
                header("White"),
                header("Black"),
                header("Result"),
                review
                    .opening
                    .as_ref()
                    .map(|o| format!("{} {}", o.eco, o.name)),
                game.moves.len() as u32,
                review.accuracy.white,
                review.accuracy.black,
                serde_json::to_string(game)?,
                serde_json::to_string(review)?,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Newest first.
    pub fn list(&self) -> Result<Vec<GameSummary>, StoreError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM games ORDER BY id DESC"
        ))?;
        let rows = statement.query_map([], summary_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get(&self, id: i64) -> Result<Option<StoredGame>, StoreError> {
        let row = self
            .conn
            .query_row(
                &format!(
                    "SELECT {SUMMARY_COLUMNS}, game_json, review_json FROM games WHERE id = ?1"
                ),
                params![id],
                |row| {
                    Ok((
                        summary_from_row(row)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                    ))
                },
            )
            .optional()?;
        let Some((summary, game_json, review_json)) = row else {
            return Ok(None);
        };
        let game: Game = serde_json::from_str(&game_json)?;
        let mut review: Review = serde_json::from_str(&review_json)?;
        // Reviews saved before commentary existed get theirs now.
        backfill_commentary(&game, &mut review);
        Ok(Some(StoredGame {
            summary,
            game,
            review,
        }))
    }

    /// Returns whether a game was removed.
    pub fn delete(&self, id: i64) -> Result<bool, StoreError> {
        Ok(self
            .conn
            .execute("DELETE FROM games WHERE id = ?1", params![id])?
            > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Limits;
    use crate::eval::{Eval, Side};
    use crate::game::parse_pgn;
    use crate::openings::Opening;
    use std::collections::BTreeMap;

    fn sample(white: &str) -> (Game, Review) {
        let pgn = format!("[White \"{white}\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n1. e4 e5 *");
        let game = parse_pgn(&pgn).unwrap().remove(0);
        let review = Review {
            headers: game.headers.clone(),
            opening: Some(Opening {
                eco: "C20".into(),
                name: "King's Pawn Game".into(),
            }),
            engine: "scripted".into(),
            limits: Limits::default(),
            evals: vec![Eval::Cp(20), Eval::Cp(-10), Eval::Cp(15)],
            moves: vec![],
            accuracy: Accuracy {
                white: Some(91.5),
                black: None,
            },
            critical_plies: vec![],
        };
        (game, review)
    }

    #[test]
    fn a_saved_game_comes_back_unchanged() {
        let store = GameStore::in_memory().unwrap();
        let (game, review) = sample("Alice");
        let id = store.save(&game, &review).unwrap();
        let stored = store.get(id).unwrap().expect("saved game exists");
        assert_eq!(stored.game, game);
        assert_eq!(stored.review, review);
        assert_eq!(stored.summary.id, id);
    }

    #[test]
    fn the_summary_describes_the_game() {
        let store = GameStore::in_memory().unwrap();
        let (game, review) = sample("Alice");
        store.save(&game, &review).unwrap();
        let summary = &store.list().unwrap()[0];
        assert_eq!(summary.white, "Alice");
        assert_eq!(summary.black, "B");
        assert_eq!(summary.result, "1-0");
        assert_eq!(summary.opening.as_deref(), Some("C20 King's Pawn Game"));
        assert_eq!(summary.moves, 2);
        assert_eq!(summary.accuracy.white, Some(91.5));
        assert_eq!(summary.accuracy.black, None);
        assert!(summary.created_at > 1_700_000_000);
    }

    #[test]
    fn games_without_headers_still_list() {
        let store = GameStore::in_memory().unwrap();
        let game = Game::from_uci_moves(None, &["e2e4".to_string()], BTreeMap::new()).unwrap();
        let (_, mut review) = sample("x");
        review.opening = None;
        store.save(&game, &review).unwrap();
        let summary = &store.list().unwrap()[0];
        assert_eq!(
            (summary.white.as_str(), summary.result.as_str()),
            ("?", "?")
        );
        assert_eq!(summary.opening, None);
    }

    #[test]
    fn the_list_is_newest_first() {
        let store = GameStore::in_memory().unwrap();
        let first = store.save(&sample("First").0, &sample("First").1).unwrap();
        let second = store
            .save(&sample("Second").0, &sample("Second").1)
            .unwrap();
        let ids: Vec<i64> = store.list().unwrap().iter().map(|s| s.id).collect();
        assert_eq!(ids, [second, first]);
    }

    #[test]
    fn unknown_ids_are_none_and_deleting_removes_the_game() {
        let store = GameStore::in_memory().unwrap();
        assert!(store.get(42).unwrap().is_none());
        let (game, review) = sample("Alice");
        let id = store.save(&game, &review).unwrap();
        assert!(store.delete(id).unwrap());
        assert!(!store.delete(id).unwrap());
        assert!(store.get(id).unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn games_survive_reopening_the_file() {
        let dir = std::env::temp_dir().join(format!("chess-analyzer-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("games.db");
        let _ = std::fs::remove_file(&path);
        let (game, review) = sample("Alice");
        let id = GameStore::open(&path)
            .unwrap()
            .save(&game, &review)
            .unwrap();
        let reopened = GameStore::open(&path).unwrap();
        assert_eq!(reopened.get(id).unwrap().unwrap().game, game);
        drop(reopened);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_review_saved_before_commentary_existed_gets_it_when_loaded() {
        let store = GameStore::in_memory().unwrap();
        let game = parse_pgn("[White \"A\"]\n[Black \"B\"]\n\n1. e4 *")
            .unwrap()
            .remove(0);
        let (_, mut review) = sample("A");
        review.moves = vec![crate::review::MoveReview {
            ply: 1,
            move_number: 1,
            side: Side::White,
            san: "e4".into(),
            uci: "e2e4".into(),
            class: crate::classify::MoveClass::Best,
            eval_before: Eval::Cp(20),
            eval_after: Eval::Cp(20),
            best_uci: Some("e2e4".into()),
            best_san: Some("e4".into()),
            best_pv: vec!["e2e4".into()],
            loss: 0.0,
            accuracy: 100.0,
            critical: false,
            commentary: None,
        }];
        let id = store.save(&game, &review).unwrap();
        // Write the JSON the way an older version did: without a commentary key at all.
        let mut value = serde_json::to_value(&review).unwrap();
        value["moves"][0]
            .as_object_mut()
            .unwrap()
            .remove("commentary");
        store
            .conn
            .execute(
                "UPDATE games SET review_json = ?1 WHERE id = ?2",
                params![value.to_string(), id],
            )
            .unwrap();
        let loaded = store.get(id).unwrap().unwrap();
        let text = loaded.review.moves[0].commentary.as_deref();
        assert!(text.is_some_and(|t| t.starts_with("e4 is ")), "{text:?}");
    }

    #[test]
    fn side_is_part_of_the_stored_review_round_trip() {
        // Eval::Checkmate carries a Side; make sure it survives the JSON column.
        let store = GameStore::in_memory().unwrap();
        let (game, mut review) = sample("Alice");
        review.evals.push(Eval::Checkmate(Side::Black));
        let id = store.save(&game, &review).unwrap();
        let back = store.get(id).unwrap().unwrap().review;
        assert_eq!(back.evals.last(), Some(&Eval::Checkmate(Side::Black)));
    }
}
