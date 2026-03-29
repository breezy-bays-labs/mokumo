// @vitest-environment jsdom

import { render, screen } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect, beforeEach, afterEach } from "vitest";
import RecoveryCodes from "./recovery-codes.svelte";

vi.mock("$app/environment", () => ({ browser: true, dev: false, building: false }));

// Must be hoisted so the factories can reference them before imports resolve
const { mockInvoke, mockToastError } = vi.hoisted(() => ({
  mockInvoke: vi.fn().mockResolvedValue(undefined),
  mockToastError: vi.fn(),
}));

// Top-level mocks — hoisted automatically by Vitest, replace the modules in all tests
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

vi.mock("$lib/components/toast", () => ({
  toast: { error: mockToastError },
}));

const MOCK_CODES = ["ABCD-1234", "EFGH-5678", "IJKL-9012", "MNOP-3456"];

describe("RecoveryCodes", () => {
  beforeEach(() => {
    mockInvoke.mockClear();
    mockToastError.mockClear();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("code display", () => {
    it("renders all recovery codes", () => {
      render(RecoveryCodes, { codes: MOCK_CODES });
      for (const code of MOCK_CODES) {
        expect(screen.getByText(code)).toBeInTheDocument();
      }
    });

    it("renders numbered list positions", () => {
      render(RecoveryCodes, { codes: MOCK_CODES });
      expect(screen.getByText("1.")).toBeInTheDocument();
      expect(screen.getByText("4.")).toBeInTheDocument();
    });
  });

  describe("print button", () => {
    it("calls window.print() in browser (non-Tauri) context", async () => {
      const printSpy = vi.spyOn(window, "print").mockImplementation(() => {});
      render(RecoveryCodes, { codes: MOCK_CODES });
      await userEvent.click(screen.getByRole("button", { name: /print/i }));
      expect(printSpy).toHaveBeenCalledOnce();
      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("invokes print_window Tauri command when running inside Tauri", async () => {
      (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
      render(RecoveryCodes, { codes: MOCK_CODES });
      await userEvent.click(screen.getByRole("button", { name: /print/i }));
      expect(mockInvoke).toHaveBeenCalledOnce();
      expect(mockInvoke).toHaveBeenCalledWith("print_window");
    });

    it("does not call window.print() in Tauri context", async () => {
      const printSpy = vi.spyOn(window, "print").mockImplementation(() => {});
      (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
      render(RecoveryCodes, { codes: MOCK_CODES });
      await userEvent.click(screen.getByRole("button", { name: /print/i }));
      expect(printSpy).not.toHaveBeenCalled();
    });

    it("shows error toast when print_window invoke fails", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("native print failed"));
      (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
      render(RecoveryCodes, { codes: MOCK_CODES });
      await userEvent.click(screen.getByRole("button", { name: /print/i }));
      expect(mockToastError).toHaveBeenCalledOnce();
      expect(mockToastError).toHaveBeenCalledWith(
        "Could not open the print dialog. Try downloading your codes instead.",
      );
    });
  });
});
