import { CLASS_INFO, CLASS_ORDER } from "../lib/classes";
import { formatAccuracy } from "../lib/format";
import type { ReviewData } from "../lib/reviewData";
import { classCounts } from "../lib/summary";

/**
 * Players, accuracy, opening and how many moves of each kind each side played. `hideOpening`
 * leaves the opening out, for a game that is still being played.
 */
export function SummaryPanel({ data, hideOpening = false }: { data: ReviewData; hideOpening?: boolean }) {
  const header = (key: string) => data.game.headers[key] ?? "?";
  const white = classCounts(data.moves, "white");
  const black = classCounts(data.moves, "black");
  const shown = CLASS_ORDER.filter((c) => (white[c] ?? 0) + (black[c] ?? 0) > 0);

  return (
    <section className="summary" aria-label="Summary">
      <div className="summary__players">
        <div className="summary__player">
          <span className="summary__name">{header("White")}</span>
          <span className="summary__accuracy" aria-label="White accuracy">
            {formatAccuracy(data.accuracy?.white ?? null)}
          </span>
        </div>
        <div className="summary__player">
          <span className="summary__name">{header("Black")}</span>
          <span className="summary__accuracy" aria-label="Black accuracy">
            {formatAccuracy(data.accuracy?.black ?? null)}
          </span>
        </div>
      </div>
      {!hideOpening && (
        <p className="summary__opening">
          {data.opening ?? (data.complete ? "Opening not in the book" : "Naming the opening once the review is done…")}
        </p>
      )}
      {shown.length > 0 && (
        <table className="summary__counts">
          <tbody>
            {shown.map((c) => (
              <tr key={c}>
                <td className="summary__count">{white[c] ?? 0}</td>
                <td className="summary__class" style={{ color: CLASS_INFO[c].color }}>
                  {CLASS_INFO[c].label}
                </td>
                <td className="summary__count">{black[c] ?? 0}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
