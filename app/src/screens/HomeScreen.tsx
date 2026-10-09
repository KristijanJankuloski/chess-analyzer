import { useCallback, useEffect, useState } from "react";
import { type Api, errorMessage } from "../api/types";
import type { GameSummary } from "../generated/GameSummary";
import type { PgnGameInfo } from "../generated/PgnGameInfo";
import type { ReviewSource } from "../generated/ReviewSource";
import { formatAccuracy, formatDate } from "../lib/format";

export interface HomeScreenProps {
  api: Api;
  onStart: (source: ReviewSource) => void;
  onOpen: (gameId: number) => void;
  /** Something to tell the user, e.g. why the last review could not start. */
  notice?: string | null;
  /** Opens Settings, where Stockfish can be downloaded. */
  onOpenSettings?: () => void;
}

function gameLabel(game: PgnGameInfo): string {
  return `${game.index + 1}. ${game.white} vs ${game.black} (${game.result}), ${Math.ceil(game.moves / 2)} moves`;
}

export function HomeScreen({ api, onStart, onOpen, notice, onOpenSettings }: HomeScreenProps) {
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [choices, setChoices] = useState<PgnGameInfo[] | null>(null);
  const [chosen, setChosen] = useState(0);
  const [recent, setRecent] = useState<GameSummary[]>([]);
  const [engineMissing, setEngineMissing] = useState(false);
  useEffect(() => {
    api.checkEngine().then(
      (status) => setEngineMissing(!status.found),
      () => undefined,
    );
  }, [api]);

  const loadRecent = useCallback(() => {
    api.listGames().then(setRecent, (e) => setError(errorMessage(e)));
  }, [api]);
  useEffect(loadRecent, [loadRecent]);

  const openFile = async () => {
    setError(null);
    try {
      const path = await api.pickPgnFile();
      if (path) {
        setText(await api.readPgnFile(path));
        setChoices(null);
      }
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const review = async () => {
    setError(null);
    try {
      const games = await api.parsePgnGames(text);
      if (games.length === 1) {
        onStart({ kind: "pgn", text, game_index: games[0].index });
      } else {
        setChoices(games);
        setChosen(games[0].index);
      }
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const remove = async (id: number) => {
    await api.deleteGame(id);
    loadRecent();
  };

  return (
    <div className="home">
      <section className="home__new">
        <h2>Review a game</h2>
        {engineMissing && (
          <p className="home__notice" role="status">
            Stockfish was not found, so games cannot be reviewed yet.
            <button type="button" onClick={onOpenSettings}>
              Get Stockfish in Settings
            </button>
          </p>
        )}
        <label htmlFor="pgn-text">Paste a PGN</label>
        <textarea
          id="pgn-text"
          aria-label="PGN text"
          value={text}
          rows={10}
          placeholder={'[White "…"]\n[Black "…"]\n\n1. e4 e5 2. Nf3 …'}
          onChange={(e) => {
            setText(e.target.value);
            setChoices(null);
          }}
        />
        <div className="home__actions">
          <button type="button" onClick={openFile}>
            Open PGN file…
          </button>
          <button type="button" className="primary" onClick={review} disabled={text.trim() === ""}>
            Review
          </button>
        </div>

        {choices && (
          <fieldset className="home__choices">
            <legend>This PGN contains {choices.length} games. Which one?</legend>
            {choices.map((game) => (
              <label key={game.index}>
                <input
                  type="radio"
                  name="game"
                  checked={chosen === game.index}
                  onChange={() => setChosen(game.index)}
                />
                {gameLabel(game)}
              </label>
            ))}
            <button
              type="button"
              className="primary"
              onClick={() => onStart({ kind: "pgn", text, game_index: chosen })}
            >
              Review selected game
            </button>
          </fieldset>
        )}

        {(error ?? notice) && (
          <p className="error" role="alert">
            {error ?? notice}
          </p>
        )}
      </section>

      <section className="home__recent">
        <h2>Recent games</h2>
        {recent.length === 0 ? (
          <p className="muted">Reviewed games will appear here.</p>
        ) : (
          <ul>
            {recent.map((game) => (
              <li key={game.id}>
                <button type="button" className="home__game" onClick={() => onOpen(game.id)}>
                  <span className="home__players">
                    {game.white} vs {game.black}
                  </span>
                  <span className="muted">
                    {game.result} · {formatDate(game.created_at)} · accuracy{" "}
                    {formatAccuracy(game.accuracy.white)} / {formatAccuracy(game.accuracy.black)}
                    {game.opening ? ` · ${game.opening}` : ""}
                  </span>
                </button>
                <button
                  type="button"
                  className="home__delete"
                  aria-label={`Delete ${game.white} vs ${game.black}`}
                  onClick={() => remove(game.id)}
                >
                  ✕
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
