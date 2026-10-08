import type { RecordDraft, RecordResult } from "../lib/record";

const RESULTS: { value: RecordResult; label: string }[] = [
  { value: "*", label: "Unfinished" },
  { value: "1-0", label: "1-0 (White won)" },
  { value: "0-1", label: "0-1 (Black won)" },
  { value: "1/2-1/2", label: "½-½ (draw)" },
];

export interface GameDetailsProps {
  /** Prefix for the field ids, so two forms on one page never clash. */
  idPrefix: string;
  white: string;
  black: string;
  result: RecordResult;
  /** Whether the game ended on the board; if so its result is fixed. */
  ended: { over: boolean; result: RecordResult };
  onChange: (patch: Partial<Pick<RecordDraft, "white" | "black" | "result">>) => void;
}

/** The players' names and the result of a game being entered by hand. */
export function GameDetails({ idPrefix, white, black, result, ended, onChange }: GameDetailsProps) {
  return (
    <div className="summary record__details">
      <label htmlFor={`${idPrefix}-white`}>White</label>
      <input
        id={`${idPrefix}-white`}
        type="text"
        value={white}
        placeholder="White"
        onChange={(e) => onChange({ white: e.target.value })}
      />
      <label htmlFor={`${idPrefix}-black`}>Black</label>
      <input
        id={`${idPrefix}-black`}
        type="text"
        value={black}
        placeholder="Black"
        onChange={(e) => onChange({ black: e.target.value })}
      />
      <label htmlFor={`${idPrefix}-result`}>Result</label>
      <select
        id={`${idPrefix}-result`}
        value={ended.over ? ended.result : result}
        disabled={ended.over}
        onChange={(e) => onChange({ result: e.target.value as RecordResult })}
      >
        {(ended.over ? RESULTS.filter((r) => r.value === ended.result) : RESULTS).map((r) => (
          <option key={r.value} value={r.value}>
            {r.label}
          </option>
        ))}
      </select>
    </div>
  );
}
