import type { InstallProgress } from "../generated/InstallProgress";

const MB = 1_000_000;

/** What to tell the user while Stockfish downloads. `null` means no news has arrived yet. */
export function describeInstall(progress: InstallProgress | null): string {
  if (!progress) return "Starting…";
  switch (progress.stage) {
    case "downloading": {
      const done = Math.round(progress.downloaded / MB);
      return progress.total
        ? `Downloading… ${done} of ${Math.round(progress.total / MB)} MB`
        : `Downloading… ${done} MB`;
    }
    case "verifying":
      return "Checking the download…";
    case "installing":
      return "Installing…";
  }
}

/** How full the progress bar is (0 to 1), or null when unknown (an indeterminate bar). */
export function installFraction(progress: InstallProgress | null): number | null {
  if (!progress) return null;
  if (progress.stage !== "downloading") return 1;
  return progress.total ? Math.min(1, progress.downloaded / progress.total) : null;
}
