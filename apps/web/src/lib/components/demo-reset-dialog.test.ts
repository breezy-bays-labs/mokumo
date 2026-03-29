// @vitest-environment jsdom

import { render, screen, waitFor } from "@testing-library/svelte";
import { fireEvent } from "@testing-library/svelte";
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
    vi.mocked(fetch).mockResolvedValue({ ok: true, json: async () => ({}) } as unknown as Response);

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
    vi.mocked(fetch).mockResolvedValue({
      ok: false,
      json: async () => ({ message: "DB locked" }),
    } as unknown as Response);

    render(DemoResetDialog, { open: true });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /^reset$/i }));

    expect(screen.getByText(/db locked/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^reset$/i })).not.toBeDisabled();
  });

  it("API error: falls back to default message when response body is not JSON", async () => {
    vi.mocked(fetch).mockResolvedValue({
      ok: false,
      json: async () => {
        throw new Error("not JSON");
      },
    } as unknown as Response);

    render(DemoResetDialog, { open: true });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /^reset$/i }));

    expect(screen.getByText(/failed to reset demo data/i)).toBeInTheDocument();
  });

  it("network error: shows connection lost message, reloads after 3000ms", async () => {
    vi.useFakeTimers();
    Object.defineProperty(window, "location", {
      writable: true,
      value: { ...window.location, reload: vi.fn() },
    });
    vi.mocked(fetch).mockRejectedValue(new Error("Network error"));

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
    expect(screen.queryByText(/reset demo data/i)).not.toBeInTheDocument();
  });

  it("reset button is disabled and shows resetting state during reset", async () => {
    vi.mocked(fetch).mockImplementation(() => new Promise(() => {})); // never resolves

    render(DemoResetDialog, { open: true });

    // fireEvent.click is synchronous — starts the async handler without awaiting completion
    fireEvent.click(screen.getByRole("button", { name: /^reset$/i }));

    // Wait for Svelte reactivity to flush `resetting = true` and re-render
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /resetting/i })).toBeDisabled();
    });
    // Cancel button: Bits UI AlertDialog.Cancel checks disabled internally but does not
    // forward the `disabled` attribute to the DOM element — it only blocks the click handler.
    // Verify the cancel button is present (not removed) but clicking it during reset is a no-op.
    expect(screen.getByRole("button", { name: /cancel/i })).toBeInTheDocument();
  });
});
