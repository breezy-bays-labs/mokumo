// @vitest-environment jsdom

import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect, beforeEach, afterEach } from "vitest";
import ForgotPasswordPage from "./+page.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));
vi.mock("$lib/api", () => ({ apiFetch: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath: vi.fn().mockResolvedValue(undefined) }));

import { apiFetch } from "$lib/api";
import { openPath } from "@tauri-apps/plugin-opener";

const mockApiFetch = vi.mocked(apiFetch);
const mockOpenPath = vi.mocked(openPath);

describe("ForgotPasswordPage", () => {
  beforeEach(() => {
    mockApiFetch.mockReset();
    mockOpenPath.mockReset();
    vi.unstubAllGlobals();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("phase: email", () => {
    it("renders email form initially", () => {
      render(ForgotPasswordPage);
      expect(screen.getByText(/forgot password/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/email/i)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /send recovery file/i })).toBeInTheDocument();
    });

    it("shows error when API returns 400 for unknown email", async () => {
      mockApiFetch.mockResolvedValue({
        ok: false,
        status: 400,
        error: {
          code: "validation_error",
          message: "No account found for that email address",
          details: null,
        },
      });

      render(ForgotPasswordPage);
      const user = userEvent.setup();

      await user.type(screen.getByLabelText(/email/i), "nobody@shop.local");
      await user.click(screen.getByRole("button", { name: /send recovery file/i }));

      await waitFor(() => {
        expect(screen.getByText(/no account found/i)).toBeInTheDocument();
      });
      // Should NOT advance to reset phase
      expect(screen.queryByLabelText(/pin/i)).not.toBeInTheDocument();
    });
  });

  describe("phase: reset", () => {
    async function advanceToResetPhase(
      recoveryFilePath = "/Users/test/Desktop/mokumo-recovery-abc.html",
    ) {
      mockApiFetch.mockResolvedValue({
        ok: true,
        status: 200,
        data: { message: "Recovery file placed", recovery_file_path: recoveryFilePath },
      });

      render(ForgotPasswordPage);
      const user = userEvent.setup();

      await user.type(screen.getByLabelText(/email/i), "admin@shop.local");
      await user.click(screen.getByRole("button", { name: /send recovery file/i }));

      await waitFor(() => {
        expect(screen.getByLabelText(/pin/i)).toBeInTheDocument();
      });

      return user;
    }

    it("step 2 description tells user the recovery file is on their Desktop", async () => {
      await advanceToResetPhase();
      expect(screen.getByText(/desktop/i)).toBeInTheDocument();
    });

    it("step 2 description includes guidance to open the recovery file", async () => {
      await advanceToResetPhase();
      expect(screen.getByText(/recovery file/i)).toBeInTheDocument();
    });

    it("opens recovery file via Tauri opener when in Tauri context", async () => {
      vi.stubGlobal("__TAURI_INTERNALS__", {});

      await advanceToResetPhase("/Users/test/Desktop/mokumo-recovery-abc.html");

      await waitFor(() => {
        expect(mockOpenPath).toHaveBeenCalledWith("/Users/test/Desktop/mokumo-recovery-abc.html");
      });
    });

    it("does not call opener when not in Tauri context", async () => {
      // __TAURI_INTERNALS__ is not set (unstubAllGlobals in beforeEach)
      await advanceToResetPhase();
      // Give time for any async calls to settle
      await new Promise((r) => setTimeout(r, 50));
      expect(mockOpenPath).not.toHaveBeenCalled();
    });
  });
});
