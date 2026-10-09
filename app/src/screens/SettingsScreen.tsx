import { useEffect, useState } from "react";
import { type Api, errorMessage } from "../api/types";
import type { EngineStatus } from "../generated/EngineStatus";
import type { InstallProgress } from "../generated/InstallProgress";
import type { Settings } from "../generated/Settings";
import { describeInstall, installFraction } from "../lib/install";

export interface SettingsScreenProps {
  api: Api;
  onDone: () => void;
}

type NumberField = "threads" | "hash_mb" | "depth" | "multipv";

// The upper limits repeat `MAX_*` in crates/core/src/settings.rs, which refuses anything above them.
const FIELDS: { key: NumberField; label: string; hint: string; max: number }[] = [
  { key: "depth", label: "Depth", hint: "How deep Stockfish searches each position. Higher is stronger and slower.", max: 60 },
  { key: "multipv", label: "Lines", hint: "Alternative moves analysed per position. Great moves need at least 2.", max: 10 },
  { key: "threads", label: "Threads", hint: "CPU threads Stockfish may use.", max: 256 },
  { key: "hash_mb", label: "Hash (MB)", hint: "Memory Stockfish may use.", max: 65536 },
];

export function SettingsScreen({ api, onDone }: SettingsScreenProps) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const [downloading, setDownloading] = useState(false);

  useEffect(() => {
    api.getSettings().then(setSettings, (e) => setMessage({ text: errorMessage(e), error: true }));
  }, [api]);

  // Know whether Stockfish works as soon as the screen opens, so a missing engine is obvious.
  useEffect(() => {
    api.checkEngine().then(setStatus, () => undefined);
  }, [api]);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let gone = false;
    api.onInstallProgress(setProgress).then((off) => {
      if (gone) off();
      else unsubscribe = off;
    });
    return () => {
      gone = true;
      unsubscribe?.();
    };
  }, [api]);

  if (!settings) {
    return <p className="muted">{message ? message.text : "Loading settings…"}</p>;
  }

  const save = async (): Promise<boolean> => {
    try {
      setSettings(await api.saveSettings(settings));
      setMessage({ text: "Saved.", error: false });
      return true;
    } catch (e) {
      setMessage({ text: errorMessage(e), error: true });
      return false;
    }
  };

  const check = async () => {
    // The engine is checked with the saved settings, so save the edited path first.
    if (await save()) setStatus(await api.checkEngine());
  };

  // The label repeats the pinned version in crates/core/src/engine_install.rs.
  const download = async () => {
    setDownloading(true);
    setProgress(null);
    setMessage(null);
    try {
      const installed = await api.downloadStockfish();
      // The app saved the new path itself; show it without touching other unsaved edits.
      setSettings((current) => current && { ...current, engine_path: installed.path });
      setStatus({ found: true, name: installed.engine, error: null });
      setMessage({ text: `Installed ${installed.engine}.`, error: false });
    } catch (e) {
      setMessage({ text: errorMessage(e), error: true });
    } finally {
      setDownloading(false);
    }
  };

  return (
    <div className="settings">
      <h2>Settings</h2>

      <label htmlFor="engine-path">Stockfish path</label>
      <input
        id="engine-path"
        type="text"
        value={settings.engine_path ?? ""}
        placeholder="Leave empty to find Stockfish automatically"
        onChange={(e) => setSettings({ ...settings, engine_path: e.target.value.trim() === "" ? null : e.target.value })}
      />
      <div className="settings__engine">
        <button type="button" onClick={check}>
          Check engine
        </button>
        {status && (
          <span className={status.found ? "ok" : "error"} role="status">
            {status.found ? `Found ${status.name}` : (status.error ?? "Engine not found")}
          </span>
        )}
      </div>
      {status && !status.found && (
        <div className="settings__download">
          <button type="button" onClick={download} disabled={downloading}>
            Download Stockfish 19
          </button>
          {downloading && (
            <>
              <progress value={installFraction(progress) ?? undefined} max={1} aria-label="Download progress" />
              <span role="status">{describeInstall(progress)}</span>
            </>
          )}
          <p className="muted">
            About 81 MB, from the official Stockfish release on GitHub. Stockfish is free software under the
            GNU GPL v3 and runs as a separate program.
          </p>
        </div>
      )}

      {FIELDS.map(({ key, label, hint, max }) => (
        <div className="settings__field" key={key}>
          <label htmlFor={`setting-${key}`}>{label}</label>
          <input
            id={`setting-${key}`}
            type="number"
            min={1}
            max={max}
            value={settings[key]}
            onChange={(e) => setSettings({ ...settings, [key]: Number(e.target.value) })}
          />
          <span className="muted">{hint}</span>
        </div>
      ))}

      <div className="settings__actions">
        <button type="button" className="primary" onClick={save}>
          Save
        </button>
        <button type="button" onClick={onDone}>
          Close
        </button>
        {message && (
          <span className={message.error ? "error" : "ok"} role="status">
            {message.text}
          </span>
        )}
      </div>
    </div>
  );
}
