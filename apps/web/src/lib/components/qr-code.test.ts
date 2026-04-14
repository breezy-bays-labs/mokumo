// @vitest-environment jsdom

import { render, screen, waitFor } from "@testing-library/svelte";
import { vi, describe, it, expect, beforeEach } from "vitest";
import QrCode from "./qr-code.svelte";

vi.mock("$app/environment", () => ({ browser: true, dev: false, building: false }));

const { mockToCanvas } = vi.hoisted(() => ({
  mockToCanvas: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("qrcode", () => ({
  default: { toCanvas: mockToCanvas },
}));

describe("QrCode", () => {
  beforeEach(() => {
    mockToCanvas.mockClear();
    mockToCanvas.mockResolvedValue(undefined);
  });

  it("renders without errors when mounted before data loads (null value)", () => {
    render(QrCode, { value: null });
    // Should not throw; canvas is in DOM but hidden
    expect(screen.getByTestId("qr-code")).toBeInTheDocument();
    expect(screen.getByTestId("qr-code-placeholder")).toBeInTheDocument();
    expect(mockToCanvas).not.toHaveBeenCalled();
  });

  it("shows placeholder when value is null", () => {
    render(QrCode, { value: null });
    expect(screen.getByTestId("qr-code-placeholder")).toBeVisible();
    expect(screen.queryByTestId("qr-code-error")).not.toBeInTheDocument();
  });

  it("renders canvas and calls toCanvas when valid value provided", async () => {
    render(QrCode, { value: "http://192.168.1.50:6565" });

    await waitFor(() => {
      expect(mockToCanvas).toHaveBeenCalledWith(
        expect.any(HTMLCanvasElement),
        "http://192.168.1.50:6565",
        expect.objectContaining({ width: 200, margin: 1 }),
      );
    });

    expect(screen.getByTestId("qr-code")).not.toHaveClass("hidden");
    expect(screen.queryByTestId("qr-code-placeholder")).not.toBeInTheDocument();
  });

  it("shows error state when toCanvas rejects", async () => {
    mockToCanvas.mockRejectedValue(new Error("No input text"));
    render(QrCode, { value: "http://192.168.1.50:6565" });

    await waitFor(() => {
      expect(screen.getByTestId("qr-code-error")).toBeVisible();
    });

    expect(screen.getByTestId("qr-code")).toHaveClass("hidden");
  });

  it("hides placeholder and error divs when value is valid and render succeeds", async () => {
    render(QrCode, { value: "http://192.168.1.50:6565" });

    await waitFor(() => {
      expect(mockToCanvas).toHaveBeenCalled();
    });

    expect(screen.queryByTestId("qr-code-placeholder")).not.toBeInTheDocument();
    expect(screen.queryByTestId("qr-code-error")).not.toBeInTheDocument();
  });
});
