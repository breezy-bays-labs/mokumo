<script lang="ts">
  import { goto } from "$app/navigation";
  import { apiFetch } from "$lib/api";
  import PasswordInput from "$lib/components/password-input.svelte";
  import { Button } from "$lib/components/ui/button";
  import {
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Alert, AlertDescription } from "$lib/components/ui/alert";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";

  let phase = $state<"email" | "reset">("email");
  let error = $state<string | null>(null);
  let loading = $state(false);
  let recoveryFilePath = $state<string | null>(null);

  // Phase 1
  let email = $state("");

  // Phase 2
  let pin = $state("");
  let newPassword = $state("");

  let emailValid = $derived(email.length > 0);
  let resetValid = $derived(pin.length === 6 && newPassword.length >= 8);

  async function handleSendRecovery(e: Event) {
    e.preventDefault();
    error = null;
    loading = true;

    const result = await apiFetch<{
      message: string;
      recovery_file_path?: string;
    }>("/api/auth/forgot-password", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email }),
    });
    loading = false;

    if (!result.ok) {
      error = result.error.message;
      return;
    }

    recoveryFilePath =
      "data" in result ? (result.data.recovery_file_path ?? null) : null;
    phase = "reset";

    if (
      recoveryFilePath &&
      typeof window !== "undefined" &&
      "__TAURI_INTERNALS__" in window
    ) {
      try {
        const { openPath } = await import("@tauri-apps/plugin-opener");
        await openPath(recoveryFilePath);
      } catch {
        // Non-fatal: user can still open the file manually from their Desktop
      }
    }
  }

  async function handleReset(e: Event) {
    e.preventDefault();
    error = null;
    loading = true;

    const result = await apiFetch("/api/auth/reset-password", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email, pin, new_password: newPassword }),
    });
    loading = false;

    if (!result.ok) {
      error = result.error.message;
      return;
    }

    goto("/login");
  }
</script>

<Card>
  <CardHeader>
    <CardTitle>
      {phase === "email" ? "Forgot Password" : "Reset Password"}
    </CardTitle>
    <CardDescription>
      {#if phase === "email"}
        Enter your email to receive a recovery file with a PIN.
      {:else}
        A recovery file has been saved to your Desktop. Open it to find your
        6-digit PIN, then choose a new password.
      {/if}
    </CardDescription>
  </CardHeader>
  <CardContent>
    {#if error}
      <Alert variant="destructive" class="mb-4">
        <CircleAlert class="h-4 w-4" />
        <AlertDescription>{error}</AlertDescription>
      </Alert>
    {/if}

    {#if phase === "email"}
      <form class="space-y-4" onsubmit={handleSendRecovery}>
        <div class="space-y-2">
          <Label for="email">Email</Label>
          <Input
            id="email"
            type="email"
            placeholder="you@example.com"
            bind:value={email}
            autocomplete="email"
            required
          />
        </div>

        <Button type="submit" class="w-full" disabled={!emailValid || loading}>
          {loading ? "Sending..." : "Send Recovery File"}
        </Button>
      </form>
    {:else}
      <form class="space-y-4" onsubmit={handleReset}>
        <div class="space-y-2">
          <Label for="pin">PIN</Label>
          <Input
            id="pin"
            type="text"
            placeholder="123456"
            bind:value={pin}
            maxlength={6}
            class="font-mono tracking-widest text-center text-lg"
            autocomplete="one-time-code"
            inputmode="numeric"
          />
        </div>

        <div class="space-y-2">
          <Label for="new-password">New password</Label>
          <PasswordInput
            id="new-password"
            bind:value={newPassword}
            showStrength
          />
        </div>

        <Button type="submit" class="w-full" disabled={!resetValid || loading}>
          {loading ? "Resetting..." : "Reset Password"}
        </Button>
      </form>
    {/if}

    <div class="mt-4 text-center">
      <a
        href="/login"
        class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground underline-offset-4 hover:underline"
      >
        <ArrowLeft class="h-3 w-3" />
        Back to sign in
      </a>
    </div>
  </CardContent>
</Card>
