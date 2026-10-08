import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, createFakeApi } from "../api/fake";
import { SettingsScreen } from "./SettingsScreen";

async function setup(options = {}) {
  const api = createFakeApi(options);
  const onDone = vi.fn();
  render(<SettingsScreen api={api} onDone={onDone} />);
  await screen.findByRole("heading", { name: "Settings" });
  return { api, onDone, user: userEvent.setup() };
}

describe("SettingsScreen", () => {
  it("shows the current settings", async () => {
    await setup();
    expect(screen.getByLabelText("Depth")).toHaveValue(DEFAULT_SETTINGS.depth);
    expect(screen.getByLabelText("Lines")).toHaveValue(DEFAULT_SETTINGS.multipv);
    expect(screen.getByLabelText("Threads")).toHaveValue(DEFAULT_SETTINGS.threads);
    expect(screen.getByLabelText("Hash (MB)")).toHaveValue(DEFAULT_SETTINGS.hash_mb);
    expect(screen.getByLabelText("Stockfish path")).toHaveValue("");
  });

  it("saves what was edited", async () => {
    const { api, user } = await setup();
    const depth = screen.getByLabelText("Depth");
    await user.clear(depth);
    await user.type(depth, "14");
    await user.type(screen.getByLabelText("Stockfish path"), "C:/sf/stockfish.exe");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("Saved.")).toBeInTheDocument();
    expect(api.calls).toContainEqual([
      "saveSettings",
      { ...DEFAULT_SETTINGS, depth: 14, engine_path: "C:/sf/stockfish.exe" },
    ]);
  });

  it("turns an emptied path back into 'look automatically'", async () => {
    const { api, user } = await setup({ settings: { ...DEFAULT_SETTINGS, engine_path: "C:/old.exe" } });
    await user.clear(screen.getByLabelText("Stockfish path"));
    await user.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved.");
    expect(api.calls).toContainEqual(["saveSettings", { ...DEFAULT_SETTINGS, engine_path: null }]);
  });

  it("shows why settings were refused", async () => {
    const { api, user } = await setup();
    api.saveSettings = async () => {
      throw "invalid settings: depth must be at least 1";
    };
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("invalid settings: depth must be at least 1")).toBeInTheDocument();
  });

  it("checks the engine after saving the path, and reports what it found", async () => {
    const { api, user } = await setup();
    await user.click(screen.getByRole("button", { name: "Check engine" }));
    expect(await screen.findByText("Found Stockfish 19")).toBeInTheDocument();
    const names = api.calls.map((c) => c[0]);
    expect(names.indexOf("saveSettings")).toBeLessThan(names.indexOf("checkEngine"));
  });

  it("reports an engine that cannot be used", async () => {
    await setup({ engine: { found: false, name: null, error: "Stockfish was not found; run scripts/setup-stockfish" } });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Check engine" }));
    expect(await screen.findByText(/Stockfish was not found/)).toBeInTheDocument();
  });

  it("does not check the engine when the settings could not be saved", async () => {
    const { api, user } = await setup();
    api.saveSettings = async () => {
      throw "nope";
    };
    await user.click(screen.getByRole("button", { name: "Check engine" }));
    await screen.findByText("nope");
    expect(api.calls.some((c) => c[0] === "checkEngine")).toBe(false);
  });

  it("closes", async () => {
    const { onDone, user } = await setup();
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(onDone).toHaveBeenCalledOnce();
  });

  it("explains itself while loading, and when loading fails", async () => {
    const api = createFakeApi();
    api.getSettings = async () => {
      throw "could not read the settings file";
    };
    render(<SettingsScreen api={api} onDone={() => {}} />);
    await waitFor(() => expect(screen.getByText("could not read the settings file")).toBeInTheDocument());
  });
});
