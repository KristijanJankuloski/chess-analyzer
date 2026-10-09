import { describe, expect, it } from "vitest";
import { describeInstall, installFraction } from "./install";

describe("describeInstall", () => {
  it("says it is starting before any news arrives", () => {
    expect(describeInstall(null)).toBe("Starting…");
  });

  it("shows megabytes downloaded against the total", () => {
    expect(describeInstall({ stage: "downloading", downloaded: 20_000_000, total: 80_000_000 })).toBe(
      "Downloading… 20 of 80 MB",
    );
  });

  it("shows only what has arrived when the size is unknown", () => {
    expect(describeInstall({ stage: "downloading", downloaded: 5_400_000, total: null })).toBe(
      "Downloading… 5 MB",
    );
  });

  it("names the later stages", () => {
    expect(describeInstall({ stage: "verifying" })).toBe("Checking the download…");
    expect(describeInstall({ stage: "installing" })).toBe("Installing…");
  });
});

describe("installFraction", () => {
  it("is the share downloaded, never above one", () => {
    expect(installFraction({ stage: "downloading", downloaded: 25, total: 100 })).toBe(0.25);
    expect(installFraction({ stage: "downloading", downloaded: 120, total: 100 })).toBe(1);
  });

  it("is unknown (an indeterminate bar) before the size is known", () => {
    expect(installFraction(null)).toBeNull();
    expect(installFraction({ stage: "downloading", downloaded: 5, total: null })).toBeNull();
  });

  it("is full once the download is done", () => {
    expect(installFraction({ stage: "verifying" })).toBe(1);
    expect(installFraction({ stage: "installing" })).toBe(1);
  });
});
