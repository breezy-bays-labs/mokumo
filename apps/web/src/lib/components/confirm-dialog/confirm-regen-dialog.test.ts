// @vitest-environment jsdom

import { render, screen, waitFor } from "@testing-library/svelte";
import userEvent from "@testing-library/user-event";
import { vi, describe, it, expect, beforeEach, type Mock } from "vitest";
import ConfirmRegenDialog from "./confirm-regen-dialog.svelte";

const DEFAULT_PROPS = {
  open: true,
  title: "Regenerate Recovery Codes",
  description: "Enter your password to confirm.",
};

describe("ConfirmRegenDialog", () => {
  let onConfirm: Mock<(password: string) => Promise<void>>;

  beforeEach(() => {
    onConfirm = vi.fn<(password: string) => Promise<void>>().mockResolvedValue(undefined);
  });

  it("pressing Enter in the password field submits the form", async () => {
    render(ConfirmRegenDialog, { ...DEFAULT_PROPS, onConfirm });
    const user = userEvent.setup();

    const input = screen.getByLabelText(/current password/i);
    await user.type(input, "correct-password");
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(onConfirm).toHaveBeenCalledWith("correct-password");
    });
  });

  it("clicking the Regenerate button submits the form", async () => {
    render(ConfirmRegenDialog, { ...DEFAULT_PROPS, onConfirm });
    const user = userEvent.setup();

    const input = screen.getByLabelText(/current password/i);
    await user.type(input, "correct-password");
    await user.click(screen.getByRole("button", { name: /regenerate/i }));

    await waitFor(() => {
      expect(onConfirm).toHaveBeenCalledWith("correct-password");
    });
  });

  it("does not submit when password field is empty", async () => {
    render(ConfirmRegenDialog, { ...DEFAULT_PROPS, onConfirm });
    const user = userEvent.setup();

    const input = screen.getByLabelText(/current password/i);
    await user.click(input);
    await user.keyboard("{Enter}");

    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("Regenerate button is disabled when password is empty", () => {
    render(ConfirmRegenDialog, { ...DEFAULT_PROPS, onConfirm });

    expect(screen.getByRole("button", { name: /regenerate/i })).toBeDisabled();
  });

  it("shows error message on failed confirmation", async () => {
    onConfirm.mockRejectedValue(new Error("Incorrect password"));
    render(ConfirmRegenDialog, { ...DEFAULT_PROPS, onConfirm });
    const user = userEvent.setup();

    await user.type(screen.getByLabelText(/current password/i), "wrong");
    await user.keyboard("{Enter}");

    await waitFor(() => {
      expect(screen.getByText(/incorrect password/i)).toBeInTheDocument();
    });
  });
});
