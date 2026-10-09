import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, createFakeApi } from "../api/fake";
import type { EngineStatus } from "../generated/EngineStatus";
import type { Installed } from "../generated/Installed";
import type { Api } from "../api/types";
import { useStockfishInstall } from "../hooks/useStockfishInstall";
import { SettingsScreen } from "./SettingsScreen";

/** Settings as App wires it up: the download state lives outside the screen. */
function Screen({ api, onDone }: { api: Api; onDone: () => void }) {
  const install = useStockfishInstall(api);
  return <SettingsScreen api={api} install={install} onDone={onDone} />;
}

async function setup(options = {}) {
  const api = createFakeApi(options);
  const onDone = vi.fn();
  render(<Screen api={api} onDone={onDone} />);
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

  it("limits each number to what the engine accepts", async () => {
    await setup();
    const limits: [string, string][] = [
      ["Depth", "60"],
      ["Lines", "10"],
      ["Threads", "256"],
      ["Hash (MB)", "65536"],
    ];
    for (const [label, max] of limits) {
      expect(screen.getByLabelText(label)).toHaveAttribute("min", "1");
      expect(screen.getByLabelText(label)).toHaveAttribute("max", max);
    }
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
    expect(names.indexOf("saveSettings")).toBeLessThan(names.lastIndexOf("checkEngine"));
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
    // Only the check made when the screen opened.
    expect(api.calls.filter((c) => c[0] === "checkEngine")).toHaveLength(1);
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
    render(<Screen api={api} onDone={() => {}} />);
    await waitFor(() => expect(screen.getByText("could not read the settings file")).toBeInTheDocument());
  });
});

const MISSING: EngineStatus = {
  found: false,
  name: null,
  error: "Stockfish was not found; run scripts/setup-stockfish or set the engine path",
};

describe("SettingsScreen, getting Stockfish", () => {
  it("offers no download when the engine works", async () => {
    await setup();
    expect(await screen.findByText("Found Stockfish 19")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Download Stockfish/ })).not.toBeInTheDocument();
  });

  it("checks the engine when it opens and offers the download if it is missing", async () => {
    await setup({ engine: MISSING });
    expect(await screen.findByRole("button", { name: "Download Stockfish 19" })).toBeEnabled();
    expect(screen.getByText(/GNU GPL/)).toBeInTheDocument();
  });

  it("downloads, shows the engine as found and fills in the path", async () => {
    const { api, user } = await setup({ engine: MISSING });
    await user.click(await screen.findByRole("button", { name: "Download Stockfish 19" }));
    expect(await screen.findByText("Found Stockfish 19")).toBeInTheDocument();
    expect(screen.getByLabelText("Stockfish path")).toHaveValue("C:/data/engines/stockfish.exe");
    expect(api.calls).toContainEqual(["downloadStockfish"]);
    expect(screen.queryByRole("button", { name: /Download Stockfish/ })).not.toBeInTheDocument();
  });

  it("shows progress while it downloads and does not allow a second click", async () => {
    const { api, user } = await setup({ engine: MISSING });
    let finish!: (installed: Installed) => void;
    api.downloadStockfish = () =>
      new Promise<Installed>((resolve) => {
        finish = resolve;
      });
    await user.click(await screen.findByRole("button", { name: "Download Stockfish 19" }));
    expect(screen.getByRole("button", { name: "Download Stockfish 19" })).toBeDisabled();
    expect(screen.getByText("Starting…")).toBeInTheDocument();

    act(() => api.emitInstall({ stage: "downloading", downloaded: 20_000_000, total: 80_000_000 }));
    expect(screen.getByText("Downloading… 20 of 80 MB")).toBeInTheDocument();
    act(() => api.emitInstall({ stage: "verifying" }));
    expect(screen.getByText("Checking the download…")).toBeInTheDocument();

    await act(async () => finish({ path: "C:/e/stockfish.exe", engine: "Stockfish 19" }));
    expect(await screen.findByText("Found Stockfish 19")).toBeInTheDocument();
  });

  it("explains a failed download and lets the user try again", async () => {
    const { user } = await setup({
      engine: MISSING,
      installError: "could not download Stockfish: connection reset",
    });
    await user.click(await screen.findByRole("button", { name: "Download Stockfish 19" }));
    expect(await screen.findByText("could not download Stockfish: connection reset")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download Stockfish 19" })).toBeEnabled();
  });

  it("keeps edits that were not saved yet", async () => {
    const { user } = await setup({ engine: MISSING });
    const depth = screen.getByLabelText("Depth");
    await user.clear(depth);
    await user.type(depth, "14");
    await user.click(await screen.findByRole("button", { name: "Download Stockfish 19" }));
    await screen.findByText("Found Stockfish 19");
    expect(screen.getByLabelText("Depth")).toHaveValue(14);
  });
});
