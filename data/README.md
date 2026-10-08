# data

- `openings.tsv`: opening names and book lines, concatenated from the `a.tsv` to `e.tsv` files of
  [lichess-org/chess-openings](https://github.com/lichess-org/chess-openings) (CC0-1.0, public domain).
  Columns: `eco`, `name`, `pgn`. To refresh it:

  ```bash
  { printf 'eco\tname\tpgn\n'; for f in a b c d e; do
      curl -sfL "https://raw.githubusercontent.com/lichess-org/chess-openings/master/$f.tsv" | tail -n +2
    done; } | tr -d '\r' > data/openings.tsv
  ```

- `fixtures/`: small games used by the golden tests. `<name>.pgn` is the game,
  `<name>.analysis.json` is Stockfish's recorded analysis of it, and `<name>.golden.txt` is the
  expected review summary. See `crates/core/tests/golden.rs` for how to re-record and update them.
