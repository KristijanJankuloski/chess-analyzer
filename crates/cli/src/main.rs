mod report;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};
use chess_analyzer_core::cache::{CachedAnalyzer, open_database};
use chess_analyzer_core::engine::{Analyzer, EngineConfig, Limits, UciEngine, locate_stockfish};
use chess_analyzer_core::game::{decode_pgn_bytes, parse_pgn};
use chess_analyzer_core::openings::OpeningBook;
use chess_analyzer_core::review::{ReviewOptions, review_game};
use chess_analyzer_core::settings::{MAX_DEPTH, MAX_HASH_MB, MAX_MULTIPV, MAX_THREADS};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "chess-analyzer",
    about = "Local chess game review powered by Stockfish"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Review a game from a PGN file (use "-" to read stdin).
    Review(ReviewArgs),
}

#[derive(clap::Args)]
struct ReviewArgs {
    /// PGN file, or "-" for stdin.
    pgn: String,
    /// Which game to review when the PGN holds several (1-based).
    #[arg(long, default_value_t = 1)]
    game: usize,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_DEPTH)))]
    depth: u32,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_MULTIPV)))]
    multipv: u32,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_THREADS)))]
    threads: u32,
    /// Stockfish hash size in MB.
    #[arg(long, default_value_t = 256, value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_HASH_MB)))]
    hash: u32,
    /// Path to the Stockfish executable (default: STOCKFISH_PATH, then engines/stockfish).
    #[arg(long)]
    engine: Option<PathBuf>,
    /// Analysis cache database.
    #[arg(long, default_value = "chess-analyzer-cache.db")]
    cache: PathBuf,
    #[arg(long)]
    no_cache: bool,
    /// Print the full review as JSON instead of text.
    #[arg(long)]
    json: bool,
}

fn read_input(source: &str) -> Result<String> {
    let bytes = if source == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .context("reading stdin")?;
        bytes
    } else {
        std::fs::read(source).with_context(|| format!("reading {source}"))?
    };
    Ok(decode_pgn_bytes(&bytes))
}

fn review(args: ReviewArgs) -> Result<()> {
    let games = parse_pgn(&read_input(&args.pgn)?)?;
    if args.game == 0 || args.game > games.len() {
        bail!(
            "--game {} is out of range: the PGN contains {} game(s)",
            args.game,
            games.len()
        );
    }
    if games.len() > 1 {
        eprintln!(
            "The PGN contains {} games; reviewing game {} (use --game to choose).",
            games.len(),
            args.game
        );
    }
    let game = &games[args.game - 1];

    let Some(path) = locate_stockfish(args.engine.as_deref()) else {
        bail!(
            "Stockfish was not found. Run scripts/setup-stockfish, set STOCKFISH_PATH, or pass --engine."
        );
    };
    let mut config = EngineConfig::new(path);
    config.threads = args.threads;
    config.hash_mb = args.hash;
    let engine: Box<dyn Analyzer> = Box::new(UciEngine::start(config)?);

    let mut analyzer: Box<dyn Analyzer> = if args.no_cache {
        engine
    } else {
        match open_database(&args.cache) {
            Ok(conn) => Box::new(CachedAnalyzer::new(engine, conn)),
            Err(e) => {
                eprintln!("warning: {e}; continuing without a cache");
                engine
            }
        }
    };

    let options = ReviewOptions {
        limits: Limits {
            depth: args.depth,
            multipv: args.multipv,
        },
        ..ReviewOptions::default()
    };
    let cancel = AtomicBool::new(false);
    let result = review_game(
        game,
        analyzer.as_mut(),
        &options,
        OpeningBook::bundled(),
        &cancel,
        |p| {
            eprint!("\rAnalysing position {}/{}", p.done, p.total);
            let _ = std::io::stderr().flush();
        },
    );
    eprintln!();
    let review = result?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&review)?);
    } else {
        print!("{}", report::render(&review));
    }
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Review(args) => review(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut argv = vec!["chess-analyzer", "review", "game.pgn"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv)
    }

    #[test]
    fn zero_depth_multipv_threads_or_hash_are_rejected() {
        for flag in ["--depth", "--multipv", "--threads", "--hash"] {
            assert!(parse(&[flag, "0"]).is_err(), "{flag} 0 should be rejected");
        }
    }

    #[test]
    fn numbers_above_what_the_engine_accepts_are_rejected() {
        for (flag, too_big) in [
            ("--depth", "61"),
            ("--multipv", "11"),
            ("--threads", "257"),
            ("--hash", "65537"),
        ] {
            assert!(
                parse(&[flag, too_big]).is_err(),
                "{flag} {too_big} should be rejected"
            );
        }
        let at_the_limits = parse(&[
            "--depth",
            "60",
            "--multipv",
            "10",
            "--threads",
            "256",
            "--hash",
            "65536",
        ]);
        assert!(at_the_limits.is_ok());
    }

    #[test]
    fn sensible_numeric_arguments_are_accepted() {
        let cli = parse(&[
            "--depth",
            "12",
            "--multipv",
            "1",
            "--threads",
            "4",
            "--hash",
            "64",
        ]);
        assert!(cli.is_ok());
    }

    #[test]
    fn a_latin_1_pgn_file_is_read() {
        let dir =
            std::env::temp_dir().join(format!("chess-analyzer-cli-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latin1.pgn");
        std::fs::write(&path, b"[White \"Mu\xf1oz\"]\n\n1. e4 *\n").unwrap();
        let text = read_input(path.to_str().unwrap()).unwrap();
        assert!(text.contains("Mu\u{f1}oz"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
