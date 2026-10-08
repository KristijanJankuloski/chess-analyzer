import { formatEval } from "../lib/format";
import type { LivePosition } from "../lib/live";

/** How many moves of each line to show; the engine's lines get long and the rest is noise. */
const MAX_MOVES = 10;

/**
 * A line of SAN moves with their numbers, as written in a score sheet: "12. Nf3 Nc6 13. Bb5",
 * or "12... Nc6 13. Bb5" when Black moves first. The number and side come from the FEN of the
 * position the line starts from.
 */
export function numberedLine(fen: string, san: string[]): string {
  const fields = fen.split(" ");
  let number = Number.parseInt(fields[5] ?? "1", 10) || 1;
  let white = fields[1] !== "b";
  const parts: string[] = [];
  san.forEach((move, i) => {
    if (white) parts.push(`${number}.`);
    else if (i === 0) parts.push(`${number}...`);
    parts.push(move);
    if (!white) number++;
    white = !white;
  });
  return parts.join(" ");
}

export interface LiveLinesProps {
  /** What the engine has said about the position on the board, or null if nothing yet. */
  position: LivePosition | null;
  /** The position on the board, so the lines can be numbered. */
  fen: string;
}

/** The engine's best lines for the position, with the depth it has reached. */
export function LiveLines({ position, fen }: LiveLinesProps) {
  if (!position) {
    return (
      <section className="live-lines" aria-label="Engine lines">
        <p className="live-lines__waiting">Waiting for the engine…</p>
      </section>
    );
  }
  const first = position.lines[0];
  const over = position.depth === 0 && first?.pv.length === 0;
  return (
    <section className="live-lines" aria-label="Engine lines">
      <h2 className="live-lines__title">
        {over ? (
          `Game over: ${formatEval(first.eval)}`
        ) : (
          <>
            Engine <span className="live-lines__depth">depth {position.depth}</span>
          </>
        )}
      </h2>
      {!over && (
        <ol className="live-lines__list">
          {position.lines.map((line) => (
            <li key={line.rank} className="live-lines__line">
              <span className="live-lines__eval">{formatEval(line.eval)}</span>
              <span className="live-lines__moves">
                {numberedLine(fen, line.pv_san.slice(0, MAX_MOVES))}
              </span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
