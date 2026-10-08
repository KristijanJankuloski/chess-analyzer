//! Opening names and book positions, from the lichess-org/chess-openings dataset (CC0).
//!
//! Positions are matched by FEN (board, side to move, castling, en passant), so
//! transpositions are recognised.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::game::parse_pgn;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opening {
    pub eco: String,
    pub name: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OpeningsError {
    #[error("openings line {line}: {reason}")]
    BadLine { line: usize, reason: String },
}

#[derive(Debug, Default)]
pub struct OpeningBook {
    /// Every position that occurs along any known line.
    book: HashSet<String>,
    /// Positions at the end of a named line.
    names: HashMap<String, Opening>,
}

/// The first four FEN fields: everything except the move counters.
fn key(fen: &str) -> String {
    fen.split(' ').take(4).collect::<Vec<_>>().join(" ")
}

impl OpeningBook {
    /// A book that recognises nothing.
    pub fn empty() -> OpeningBook {
        OpeningBook::default()
    }

    /// Parses tab-separated `eco<TAB>name<TAB>pgn` rows. The first row is a header.
    pub fn from_tsv(text: &str) -> Result<OpeningBook, OpeningsError> {
        let mut book = OpeningBook::default();
        for (i, row) in text.lines().enumerate().skip(1) {
            if row.trim().is_empty() {
                continue;
            }
            let line = i + 1;
            let mut cols = row.split('\t');
            let (Some(eco), Some(name), Some(pgn)) = (cols.next(), cols.next(), cols.next()) else {
                return Err(OpeningsError::BadLine {
                    line,
                    reason: "expected 3 columns".into(),
                });
            };
            let games = parse_pgn(pgn).map_err(|e| OpeningsError::BadLine {
                line,
                reason: e.to_string(),
            })?;
            let game = &games[0];
            for fen in &game.positions {
                book.book.insert(key(fen));
            }
            let last = key(game.positions.last().expect("at least the start position"));
            book.names.entry(last).or_insert_with(|| Opening {
                eco: eco.to_string(),
                name: name.to_string(),
            });
        }
        Ok(book)
    }

    /// The dataset bundled into the binary (`data/openings.tsv`).
    pub fn bundled() -> &'static OpeningBook {
        static BOOK: OnceLock<OpeningBook> = OnceLock::new();
        BOOK.get_or_init(|| {
            OpeningBook::from_tsv(include_str!("../../../data/openings.tsv"))
                .expect("bundled openings.tsv is valid")
        })
    }

    pub fn is_book(&self, fen: &str) -> bool {
        self.book.contains(&key(fen))
    }

    pub fn name_of(&self, fen: &str) -> Option<&Opening> {
        self.names.get(&key(fen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TSV: &str = "eco\tname\tpgn\n\
        B00\tKing's Pawn\t1. e4\n\
        C20\tKing's Pawn Game\t1. e4 e5\n\
        C44\tKing's Knight Opening\t1. e4 e5 2. Nf3\n";

    fn fens(pgn: &str) -> Vec<String> {
        parse_pgn(pgn).unwrap()[0].positions.clone()
    }

    #[test]
    fn every_position_along_a_line_is_book() {
        let book = OpeningBook::from_tsv(TSV).unwrap();
        for fen in fens("1. e4 e5 2. Nf3") {
            assert!(book.is_book(&fen), "{fen}");
        }
        assert!(!book.is_book(&fens("1. e4 e5 2. Nf3 Nc6")[4]));
    }

    #[test]
    fn names_are_attached_to_the_end_of_a_line() {
        let book = OpeningBook::from_tsv(TSV).unwrap();
        let positions = fens("1. e4 e5 2. Nf3");
        assert_eq!(book.name_of(&positions[1]).unwrap().name, "King's Pawn");
        assert_eq!(book.name_of(&positions[3]).unwrap().eco, "C44");
        assert!(book.name_of(&positions[0]).is_none());
    }

    #[test]
    fn move_counters_do_not_matter() {
        let book = OpeningBook::from_tsv(TSV).unwrap();
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 17 99";
        assert!(book.is_book(fen));
    }

    #[test]
    fn transpositions_are_recognised() {
        let tsv = "eco\tname\tpgn\nD00\tQueen's Pawn\t1. d4 d5 2. Nf3\n";
        let book = OpeningBook::from_tsv(tsv).unwrap();
        let transposed = fens("1. Nf3 d5 2. d4");
        assert!(book.is_book(&transposed[3]));
        assert_eq!(book.name_of(&transposed[3]).unwrap().name, "Queen's Pawn");
    }

    #[test]
    fn malformed_rows_are_reported_with_their_line() {
        let err = OpeningBook::from_tsv("eco\tname\tpgn\nA00\tonly two columns\n").unwrap_err();
        assert!(matches!(err, OpeningsError::BadLine { line: 2, .. }));
        let err = OpeningBook::from_tsv("eco\tname\tpgn\nA00\tBad\t1. e4 e4\n").unwrap_err();
        assert!(matches!(err, OpeningsError::BadLine { line: 2, .. }));
    }

    #[test]
    fn bundled_dataset_loads_and_knows_the_ruy_lopez() {
        let book = OpeningBook::bundled();
        let positions = fens("1. e4 e5 2. Nf3 Nc6 3. Bb5");
        let opening = book.name_of(&positions[5]).expect("Ruy Lopez is named");
        assert!(opening.name.starts_with("Ruy Lopez"), "{}", opening.name);
        assert!(opening.eco.starts_with('C'));
    }
}
