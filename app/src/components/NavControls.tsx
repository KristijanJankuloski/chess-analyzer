import { useEffect } from "react";

export interface NavControlsProps {
  ply: number;
  /** The last ply of the game. */
  last: number;
  onSelect: (ply: number) => void;
  onFlip: () => void;
  showBest: boolean;
  onToggleBest: () => void;
  /** What the toggle is called. Defaults to "Best move". */
  toggleLabel?: string;
}

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

/** First / previous / next / last buttons, with the arrow keys as shortcuts. */
export function NavControls({
  ply,
  last,
  onSelect,
  onFlip,
  showBest,
  onToggleBest,
  toggleLabel = "Best move",
}: NavControlsProps) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
      if (isTyping(event.target)) return;
      const target =
        event.key === "ArrowLeft"
          ? ply - 1
          : event.key === "ArrowRight"
            ? ply + 1
            : event.key === "Home"
              ? 0
              : event.key === "End"
                ? last
                : null;
      if (target === null) return;
      event.preventDefault();
      onSelect(Math.min(Math.max(target, 0), last));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ply, last, onSelect]);

  return (
    <div className="nav-controls" role="toolbar" aria-label="Move navigation">
      <button type="button" onClick={() => onSelect(0)} disabled={ply <= 0} aria-label="First position">
        ⏮
      </button>
      <button type="button" onClick={() => onSelect(ply - 1)} disabled={ply <= 0} aria-label="Previous move">
        ◀
      </button>
      <button type="button" onClick={() => onSelect(ply + 1)} disabled={ply >= last} aria-label="Next move">
        ▶
      </button>
      <button type="button" onClick={() => onSelect(last)} disabled={ply >= last} aria-label="Last position">
        ⏭
      </button>
      <span className="nav-controls__spacer" />
      <button type="button" onClick={onFlip} aria-label="Flip board">
        ⇅
      </button>
      <label className="nav-controls__toggle">
        <input type="checkbox" checked={showBest} onChange={onToggleBest} />
        {toggleLabel}
      </label>
    </div>
  );
}
