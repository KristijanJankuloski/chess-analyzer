import type { Eval } from "../generated/Eval";
import type { MoveReview } from "../generated/MoveReview";
import { CLASS_INFO, isError } from "../lib/classes";
import { whiteWinPercent } from "../lib/format";

export interface EvalGraphProps {
  /** One slot per position; null where the engine has not got to yet. */
  evals: (Eval | null)[];
  /** One slot per half-move. Used to mark the moves worth a look. */
  moves: (MoveReview | null)[];
  selectedPly: number;
  onSelect: (ply: number) => void;
}

const WIDTH = 300;
const HEIGHT = 100;

function x(index: number, count: number): number {
  return count <= 1 ? 0 : (index / (count - 1)) * WIDTH;
}

function y(value: Eval): number {
  return HEIGHT - whiteWinPercent(value);
}

/** White's winning chances through the game. Click anywhere to jump to that position. */
export function EvalGraph({ evals, moves, selectedPly, onSelect }: EvalGraphProps) {
  const count = evals.length;
  const known = evals.flatMap((value, index) => (value ? [{ index, value }] : []));
  const line = known.map(({ index, value }) => `${x(index, count)},${y(value)}`);
  const area = known.length
    ? [`${x(known[0].index, count)},${HEIGHT}`, ...line, `${x(known[known.length - 1].index, count)},${HEIGHT}`].join(" ")
    : "";
  const band = count <= 1 ? WIDTH : WIDTH / (count - 1);

  return (
    <svg
      className="eval-graph"
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="none"
      role="group"
      aria-label="Evaluation graph"
    >
      <rect x={0} y={0} width={WIDTH} height={HEIGHT} className="eval-graph__black" />
      {area && <polygon points={area} className="eval-graph__white" />}
      <line x1={0} x2={WIDTH} y1={HEIGHT / 2} y2={HEIGHT / 2} className="eval-graph__mid" />
      {moves.map((move, i) => {
        const after = evals[i + 1];
        if (!move || !after || !(isError(move.class) || move.class === "brilliant" || move.class === "great")) {
          return null;
        }
        return (
          <circle
            key={`mark-${move.ply}`}
            cx={x(i + 1, count)}
            cy={y(after)}
            r={2.2}
            fill={CLASS_INFO[move.class].color}
            vectorEffect="non-scaling-stroke"
          >
            <title>{`${move.san} ${CLASS_INFO[move.class].label}`}</title>
          </circle>
        );
      })}
      <line
        x1={x(selectedPly, count)}
        x2={x(selectedPly, count)}
        y1={0}
        y2={HEIGHT}
        className="eval-graph__cursor"
        data-testid="graph-cursor"
      />
      {evals.map((_, index) => (
        <rect
          key={`hit-${index}`}
          x={x(index, count) - band / 2}
          y={0}
          width={band}
          height={HEIGHT}
          className="eval-graph__hit"
          role="button"
          aria-label={index === 0 ? "Go to the start" : `Go to move ${index}`}
          onClick={() => onSelect(index)}
        />
      ))}
    </svg>
  );
}
