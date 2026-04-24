// @vitest-environment jsdom

import { render, screen } from "@testing-library/svelte";
import { describe, it, expect, beforeEach, vi } from "vitest";
import VersionMismatchBanner from "./version-mismatch-banner.svelte";
import { versionCheck } from "$lib/stores/version-check.svelte";

vi.mock("$app/environment", () => ({ browser: true, dev: false, building: false }));

describe("VersionMismatchBanner", () => {
  beforeEach(() => {
    versionCheck.state = { status: "pending" };
  });

  it("renders banner when api_version drifts between UI and server", () => {
    versionCheck.state = { status: "mismatch", uiVersion: "1.0.0", serverVersion: "2.0.0" };
    render(VersionMismatchBanner);
    const banner = screen.getByTestId("version-mismatch-banner");
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveTextContent("1.0.0");
    expect(banner).toHaveTextContent("2.0.0");
    expect(banner.textContent?.replace(/\s+/g, " ")).toContain(
      "Re-run your installer or contact support",
    );
  });

  it("does not render on match", () => {
    versionCheck.state = { status: "match", serverVersion: "1.0.0" };
    render(VersionMismatchBanner);
    expect(screen.queryByTestId("version-mismatch-banner")).not.toBeInTheDocument();
  });

  it("does not render while the check is pending", () => {
    versionCheck.state = { status: "pending" };
    render(VersionMismatchBanner);
    expect(screen.queryByTestId("version-mismatch-banner")).not.toBeInTheDocument();
  });

  it("does not render when the endpoint is unreachable (avoids false-positive on network hiccup)", () => {
    versionCheck.state = { status: "unreachable" };
    render(VersionMismatchBanner);
    expect(screen.queryByTestId("version-mismatch-banner")).not.toBeInTheDocument();
  });
});
