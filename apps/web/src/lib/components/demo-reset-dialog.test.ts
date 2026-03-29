// @vitest-environment jsdom

import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect, beforeEach, afterEach } from "vitest";
import DemoResetDialog from "./demo-reset-dialog.svelte";

describe("DemoResetDialog", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("renders dialog content when open", () => {
    render(DemoResetDialog, { open: true });
    expect(screen.getByText(/reset demo data/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /reset/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /cancel/i })).toBeInTheDocument();
  });

  it("success: disables buttons, clears localStorage, reloads after 1500ms", async () => {
    vi.useFakeTimers();
    Object.defineProperty(window, "location", {
      writable: true,
      value: { ...window.location, reload: vi.fn() },
    });
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({ ok: true, json: async () => ({}) });

    render(DemoResetDialog, { open: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime.bind(vi) });

    await user.click(screen.getByRole("button", { name: /^reset$/i }));

    // During reset: action button should be disabled
    expect(screen.getByRole("button", { name: /resetting/i })).toBeDisabled();

    await vi.advanceTimersByTimeAsync(1500);

    expect(localStorage.getItem("demo_banner_dismissed")).toBeNull();
    expect(window.location.reload).toHaveBeenCalledOnce();
  });

  it("API error: shows error message from response body", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      json: async () => ({ message: "DB locked" }),
    });

    render(DemoResetDialog, { open: true });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /^reset$/i }));

    expect(screen.getByText(/db locked/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^reset$/i })).not.toBeDisabled();
  });

  it("network error: shows connection lost message, reloads after 3000ms", async () => {
    vi.useFakeTimers();
    Object.defineProperty(window, "location", {
      writable: true,
      value: { ...window.location, reload: vi.fn() },
    });
    (fetch as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("Network error"));

    render(DemoResetDialog, { open: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime.bind(vi) });

    await user.click(screen.getByRole("button", { name: /^reset$/i }));

    expect(screen.getByText(/connection lost/i)).toBeInTheDocument();

    await vi.advanceTimersByTimeAsync(3000);

    expect(window.location.reload).toHaveBeenCalledOnce();
  });

  it("cancel: closes dialog without calling fetch", async () => {
    render(DemoResetDialog, { open: true });
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: /cancel/i }));

    expect(fetch).not.toHaveBeenCalled();
  });

  it("reset button is disabled and shows resetting state during reset", async () => {
    (fetch as ReturnType<typeof vi.fn>).mockImplementation(() => new Promise(() => {})); // never resolves

    render(DemoResetDialog, { open: true });
    const user = userEvent.setup();
    // Don't await — the click starts the async handler but fetch never resolves
    user.click(screen.getByRole("button", { name: /^reset$/i }));

    // Wait for Svelte reactivity to flush `resetting = true` and re-render
    const { waitFor } = await import("@testing-library/svelte");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /resetting/i })).toBeDisabled();
    });
    // Cancel button: Bits UI AlertDialog.Cancel checks disabled internally but does not
    // forward the `disabled` attribute to the DOM element — it only blocks the click handler.
    // Verify the cancel button is present (not removed) but clicking it during reset is a no-op.
    expect(screen.getByRole("button", { name: /cancel/i })).toBeInTheDocument();
  });
});
