//! Games: PGN parsing and construction from a list of moves.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::ops::ControlFlow;

use pgn_reader::{RawTag, Reader, SanPlus, Visitor};
use serde::{Deserialize, Serialize};
use shakmaty::fen::Fen;
use shakmaty::san::SanPlus as ShakSanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, EnPassantMode};
use thiserror::Error;

use crate::eval::Side;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GameError {
    #[error("no game found in the PGN")]
    Empty,
    #[error("could not read the PGN: {0}")]
    Read(String),
    #[error("invalid FEN: {0}")]
    InvalidFen(String),
    #[error("illegal or unreadable move {mv:?} at ply {ply}")]
    IllegalMove { ply: usize, mv: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameMove {
    pub san: String,
    pub uci: String,
}

/// A single linear game. `positions[i]` is the FEN before `moves[i]`;
/// the last entry is the final position, so `positions.len() == moves.len() + 1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub headers: BTreeMap<String, String>,
    pub positions: Vec<String>,
    pub moves: Vec<GameMove>,
}

impl Game {
    /// The side that makes `moves[index]`.
    pub fn side_to_move(&self, index: usize) -> Side {
        if self.positions[index].split(' ').nth(1) == Some("b") {
            Side::Black
        } else {
            Side::White
        }
    }

    /// Parses the position at `index` (0 = start, `moves.len()` = final).
    pub fn position(&self, index: usize) -> Chess {
        parse_fen(&self.positions[index]).expect("positions are produced from valid FENs")
    }

    /// Builds a game from UCI moves, e.g. `["e2e4", "e7e5"]`. Used for hand-recorded games.
    pub fn from_uci_moves(
        start_fen: Option<&str>,
        uci_moves: &[String],
        headers: BTreeMap<String, String>,
    ) -> Result<Game, GameError> {
        let start = match start_fen {
            Some(fen) => parse_fen(fen)?,
            None => Chess::default(),
        };
        let mut builder = Builder::new(start, headers);
        for (i, text) in uci_moves.iter().enumerate() {
            let ply = i + 1;
            let uci = UciMove::from_ascii(text.as_bytes()).map_err(|_| GameError::IllegalMove {
                ply,
                mv: text.clone(),
            })?;
            let mv = uci
                .to_move(&builder.pos)
                .map_err(|_| GameError::IllegalMove {
                    ply,
                    mv: text.clone(),
                })?;
            builder.push(mv);
        }
        Ok(builder.finish())
    }
}

/// Parses the games in the PGN text. Variations and comments are ignored. Games without any
/// moves (including whatever the lenient reader makes of non-PGN text) are skipped, so a
/// successful result always holds at least one game with a move.
pub fn parse_pgn(text: &str) -> Result<Vec<Game>, GameError> {
    let mut reader = Reader::new(Cursor::new(text.as_bytes()));
    let mut games = Vec::new();
    let mut visitor = GameVisitor;
    while let Some(result) = reader
        .read_game(&mut visitor)
        .map_err(|e| GameError::Read(e.to_string()))?
    {
        games.push(result?);
    }
    games.retain(|game| !game.moves.is_empty());
    if games.is_empty() {
        return Err(GameError::Empty);
    }
    Ok(games)
}

fn parse_fen(fen: &str) -> Result<Chess, GameError> {
    fen.parse::<Fen>()
        .map_err(|e| GameError::InvalidFen(e.to_string()))?
        .into_position(CastlingMode::Standard)
        .map_err(|e| GameError::InvalidFen(e.to_string()))
}

fn fen_string(pos: &Chess) -> String {
    Fen::from_position(pos, EnPassantMode::Legal).to_string()
}

struct Builder {
    pos: Chess,
    headers: BTreeMap<String, String>,
    positions: Vec<String>,
    moves: Vec<GameMove>,
}

impl Builder {
    fn new(start: Chess, headers: BTreeMap<String, String>) -> Builder {
        let positions = vec![fen_string(&start)];
        Builder {
            pos: start,
            headers,
            positions,
            moves: Vec::new(),
        }
    }

    fn push(&mut self, mv: shakmaty::Move) {
        let uci = mv.to_uci(CastlingMode::Standard).to_string();
        let san = ShakSanPlus::from_move_and_play_unchecked(&mut self.pos, mv).to_string();
        self.positions.push(fen_string(&self.pos));
        self.moves.push(GameMove { san, uci });
    }

    fn finish(self) -> Game {
        Game {
            headers: self.headers,
            positions: self.positions,
            moves: self.moves,
        }
    }
}

struct GameVisitor;

struct Tags {
    headers: BTreeMap<String, String>,
    start: Option<Chess>,
    error: Option<GameError>,
}

struct Movetext {
    builder: Option<Builder>,
    error: Option<GameError>,
}

impl Visitor for GameVisitor {
    type Tags = Tags;
    type Movetext = Movetext;
    type Output = Result<Game, GameError>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(Tags {
            headers: BTreeMap::new(),
            start: None,
            error: None,
        })
    }

    fn tag(
        &mut self,
        tags: &mut Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        let name = String::from_utf8_lossy(name).into_owned();
        let value = value.decode_utf8_lossy().into_owned();
        if name == "FEN" {
            match parse_fen(&value) {
                Ok(pos) => tags.start = Some(pos),
                Err(e) => tags.error = Some(e),
            }
        }
        tags.headers.insert(name, value);
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, tags: Tags) -> ControlFlow<Self::Output, Movetext> {
        if let Some(error) = tags.error {
            return ControlFlow::Continue(Movetext {
                builder: None,
                error: Some(error),
            });
        }
        let start = tags.start.unwrap_or_default();
        ControlFlow::Continue(Movetext {
            builder: Some(Builder::new(start, tags.headers)),
            error: None,
        })
    }

    fn san(&mut self, movetext: &mut Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        let Some(builder) = movetext.builder.as_mut() else {
            return ControlFlow::Continue(());
        };
        let ply = builder.moves.len() + 1;
        match san_plus.san.to_move(&builder.pos) {
            Ok(mv) => {
                builder.push(mv);
                ControlFlow::Continue(())
            }
            Err(_) => ControlFlow::Break(Err(GameError::IllegalMove {
                ply,
                mv: san_plus.to_string(),
            })),
        }
    }

    fn end_game(&mut self, movetext: Movetext) -> Self::Output {
        match (movetext.error, movetext.builder) {
            (Some(error), _) => Err(error),
            (None, Some(builder)) => Ok(builder.finish()),
            (None, None) => Err(GameError::Empty),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::Position;

    const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn parses_a_simple_game_with_headers() {
        let pgn =
            "[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n";
        let games = parse_pgn(pgn).unwrap();
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.headers["White"], "Alice");
        assert_eq!(g.moves.len(), 4);
        assert_eq!(g.positions.len(), 5);
        assert_eq!(g.positions[0], START_FEN);
        assert_eq!(
            g.moves[0],
            GameMove {
                san: "e4".into(),
                uci: "e2e4".into()
            }
        );
        assert_eq!(g.moves[2].san, "Nf3");
        assert_eq!(g.moves[2].uci, "g1f3");
    }

    #[test]
    fn side_to_move_alternates() {
        let g = &parse_pgn("1. e4 e5 *").unwrap()[0];
        assert_eq!(g.side_to_move(0), Side::White);
        assert_eq!(g.side_to_move(1), Side::Black);
    }

    #[test]
    fn castling_and_promotion_use_standard_uci() {
        let pgn = "1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. O-O Nf6 *";
        let g = &parse_pgn(pgn).unwrap()[0];
        assert_eq!(g.moves[6].san, "O-O");
        assert_eq!(g.moves[6].uci, "e1g1");

        let promo = "[FEN \"7k/P7/8/8/8/8/8/K7 w - - 0 1\"]\n\n1. a8=Q+ *";
        let g = &parse_pgn(promo).unwrap()[0];
        assert_eq!(g.moves[0].uci, "a7a8q");
        assert_eq!(g.moves[0].san, "a8=Q+");
    }

    #[test]
    fn variations_and_comments_are_ignored() {
        let pgn = "1. e4 {best by test} e5 (1... c5 2. Nf3) 2. Nf3 *";
        let g = &parse_pgn(pgn).unwrap()[0];
        let sans: Vec<_> = g.moves.iter().map(|m| m.san.as_str()).collect();
        assert_eq!(sans, ["e4", "e5", "Nf3"]);
    }

    #[test]
    fn multiple_games_are_all_returned() {
        let pgn = "[Event \"A\"]\n\n1. e4 *\n\n[Event \"B\"]\n\n1. d4 d5 *\n";
        let games = parse_pgn(pgn).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[1].headers["Event"], "B");
        assert_eq!(games[1].moves.len(), 2);
    }

    #[test]
    fn illegal_move_reports_the_ply() {
        let err = parse_pgn("1. e4 e5 2. Ke3 *").unwrap_err();
        assert_eq!(
            err,
            GameError::IllegalMove {
                ply: 3,
                mv: "Ke3".into()
            }
        );
    }

    #[test]
    fn empty_input_is_an_error() {
        assert_eq!(parse_pgn("   \n").unwrap_err(), GameError::Empty);
    }

    #[test]
    fn invalid_fen_header_is_an_error() {
        let err = parse_pgn("[FEN \"not a fen\"]\n\n1. e4 *").unwrap_err();
        assert!(matches!(err, GameError::InvalidFen(_)));
    }

    #[test]
    fn checkmate_game_ends_in_a_final_position() {
        let g = &parse_pgn("1. f3 e5 2. g4 Qh4# 0-1").unwrap()[0];
        assert_eq!(g.moves.len(), 4);
        assert!(g.position(4).is_checkmate());
        assert_eq!(g.moves[3].san, "Qh4#");
    }

    #[test]
    fn garbage_text_is_an_error_not_an_empty_game() {
        assert_eq!(
            parse_pgn("hello, this is not chess at all"),
            Err(GameError::Empty)
        );
        assert_eq!(
            parse_pgn("<html><body>404</body></html>"),
            Err(GameError::Empty)
        );
    }

    #[test]
    fn windows_line_endings_are_accepted() {
        let pgn = "[White \"Alice\"]\r\n[Black \"Bob\"]\r\n\r\n1. e4 e5\r\n2. Nf3 Nc6 *\r\n";
        let g = &parse_pgn(pgn).unwrap()[0];
        assert_eq!(g.headers["Black"], "Bob");
        assert_eq!(g.moves.len(), 4);
    }

    #[test]
    fn non_ascii_player_names_survive() {
        let pgn = "[White \"Zoë Müller\"]\n[Black \"李雷\"]\n\n1. e4 *\n";
        let g = &parse_pgn(pgn).unwrap()[0];
        assert_eq!(g.headers["White"], "Zoë Müller");
        assert_eq!(g.headers["Black"], "李雷");
    }

    #[test]
    fn a_utf8_byte_order_mark_at_the_start_is_tolerated() {
        let pgn = "\u{feff}[White \"Alice\"]\n\n1. e4 e5 *\n";
        let g = &parse_pgn(pgn).unwrap()[0];
        assert_eq!(g.moves.len(), 2);
    }

    #[test]
    fn games_without_moves_are_skipped() {
        let pgn = "[Event \"A\"]\n\n*\n\n[Event \"B\"]\n\n1. e4 *\n";
        let games = parse_pgn(pgn).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].headers["Event"], "B");
    }
    #[test]
    fn builds_a_game_from_uci_moves() {
        let moves: Vec<String> = ["e2e4", "e7e5", "g1f3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let g = Game::from_uci_moves(None, &moves, BTreeMap::new()).unwrap();
        assert_eq!(
            g.moves.iter().map(|m| m.san.as_str()).collect::<Vec<_>>(),
            ["e4", "e5", "Nf3"]
        );
        assert_eq!(g.positions.len(), 4);
    }

    #[test]
    fn from_uci_moves_rejects_illegal_moves() {
        let moves = vec!["e2e5".to_string()];
        let err = Game::from_uci_moves(None, &moves, BTreeMap::new()).unwrap_err();
        assert_eq!(
            err,
            GameError::IllegalMove {
                ply: 1,
                mv: "e2e5".into()
            }
        );
    }
}
