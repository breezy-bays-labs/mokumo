// @vitest-environment jsdom

import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect } from "vitest";
import DemoBanner from "./demo-banner.svelte";
import { profile } from "$lib/stores/profile.svelte";

vi.mock("$app/environment", () => ({ browser: true, dev: false, building: false }));

describe("DemoBanner", () => {
  it('shows banner when setupMode is "demo"', () => {
    render(DemoBanner, { setupMode: "demo", hasProductionShop: false });
    expect(screen.getByTestId("demo-banner")).toBeInTheDocument();
  });

  it('hides banner when setupMode is not "demo"', () => {
    render(DemoBanner, { setupMode: null, hasProductionShop: false });
    expect(screen.queryByTestId("demo-banner")).not.toBeInTheDocument();
  });

  it('shows "Set Up My Shop" CTA when production is not configured', () => {
    render(DemoBanner, { setupMode: "demo", hasProductionShop: false });
    expect(screen.getByTestId("demo-banner-cta")).toHaveTextContent("Set Up My Shop");
  });

  it('shows "Go to My Shop" CTA when production is configured', () => {
    render(DemoBanner, { setupMode: "demo", hasProductionShop: true });
    expect(screen.getByTestId("demo-banner-cta")).toHaveTextContent("Go to My Shop");
  });

  it("has no dismiss button", () => {
    render(DemoBanner, { setupMode: "demo", hasProductionShop: false });
    expect(screen.queryByRole("button", { name: /dismiss/i })).not.toBeInTheDocument();
  });

  it("clicking CTA sets profile.openProfileSwitcher to true", async () => {
    profile.openProfileSwitcher = false;
    render(DemoBanner, { setupMode: "demo", hasProductionShop: false });
    const user = userEvent.setup();
    await user.click(screen.getByTestId("demo-banner-cta"));
    expect(profile.openProfileSwitcher).toBe(true);
  });
});
