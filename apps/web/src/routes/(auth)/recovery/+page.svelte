<script lang="ts">
  import { goto } from "$app/navigation";
  import { apiFetch } from "$lib/api";
  import PasswordInput from "$lib/components/password-input.svelte";
  import { toast } from "$lib/components/toast";
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

  let email = $state("");
  let recoveryCode = $state("");
  let newPassword = $state("");
  let error = $state<string | null>(null);
  let loading = $state(false);

  let isValid = $derived(
    email.length > 0 &&
      recoveryCode.trim().length > 0 &&
      newPassword.length >= 8,
  );

  async function handleSubmit(e: Event) {
    e.preventDefault();
    error = null;
    loading = true;

    const result = await apiFetch("/api/auth/recover", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        email,
        recovery_code: recoveryCode.trim(),
        new_password: newPassword,
      }),
    });
    loading = false;

    if (!result.ok) {
      error = result.error.message;
      return;
    }

    toast.info("You used a recovery code. Consider regenerating your codes.", {
      duration: 8000,
      action: {
        label: "Regenerate Codes",
        onClick: () => goto("/settings/account?regen=true"),
      },
    });
    goto("/login");
  }
</script>

<Card>
  <CardHeader>
    <CardTitle>Account Recovery</CardTitle>
    <CardDescription>
      Use one of your recovery codes to reset your password.
    </CardDescription>
  </CardHeader>
  <CardContent>
    {#if error}
      <Alert variant="destructive" class="mb-4">
        <CircleAlert class="h-4 w-4" />
        <AlertDescription>{error}</AlertDescription>
      </Alert>
    {/if}

    <form class="space-y-4" onsubmit={handleSubmit}>
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

      <div class="space-y-2">
        <Label for="recovery-code">Recovery code</Label>
        <Input
          id="recovery-code"
          type="text"
          placeholder="xxxx-xxxx"
          bind:value={recoveryCode}
          class="font-mono tracking-wide"
          autocomplete="off"
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

      <Button type="submit" class="w-full" disabled={!isValid || loading}>
        {loading ? "Resetting..." : "Reset Password"}
      </Button>
    </form>

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
