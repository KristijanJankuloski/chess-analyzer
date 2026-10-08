import { useEffect, useState } from "react";
import { type Api, errorMessage } from "../api/types";
import type { EngineStatus } from "../generated/EngineStatus";
import type { Settings } from "../generated/Settings";

export interface SettingsScreenProps {
  api: Api;
  onDone: () => void;
}

type NumberField = "threads" | "hash_mb" | "depth" | "multipv";

const FIELDS: { key: NumberField; label: string; hint: string }[] = [
  { key: "depth", label: "Depth", hint: "How deep Stockfish searches each position. Higher is stronger and slower." },
  { key: "multipv", label: "Lines", hint: "Alternative moves analysed per position. Great moves need at least 2." },
  { key: "threads", label: "Threads", hint: "CPU threads Stockfish may use." },
  { key: "hash_mb", label: "Hash (MB)", hint: "Memory Stockfish may use." },
];

export function SettingsScreen({ api, onDone }: SettingsScreenProps) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  useEffect(() => {
    api.getSettings().then(setSettings, (e) => setMessage({ text: errorMessage(e), error: true }));
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

      {FIELDS.map(({ key, label, hint }) => (
        <div className="settings__field" key={key}>
          <label htmlFor={`setting-${key}`}>{label}</label>
          <input
            id={`setting-${key}`}
            type="number"
            min={1}
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
