<script lang="ts">
  import WizardProgress, { type WizardStep } from "$lib/components/WizardProgress.svelte";
  import {
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Label } from "$lib/components/ui/label/index.js";

  let { data } = $props();
  let branding = $derived(data.branding);

  type StepId = "request-pin" | "enter-pin" | "new-password";

  const STEPS: WizardStep[] = [
    { id: "request-pin", label: "Request PIN" },
    { id: "enter-pin", label: "Enter PIN" },
    { id: "new-password", label: "New password" },
  ];

  let currentStep = $state<StepId>("request-pin");

  let recoveryEmail = $state("");
  let pinValue = $state("");
  let newPassword = $state("");

  let strengthError = $derived.by(() => {
    if (newPassword === "") return null;
    if (newPassword.length < 12) return "Password must be at least 12 characters";
    if (!/\d/.test(newPassword)) return "Password must include at least one number";
    return null;
  });

  let canSubmitNewPassword = $derived(newPassword.length >= 12 && /\d/.test(newPassword));

  function selectStep(id: string): void {
    currentStep = id as StepId;
  }

  async function handleRequestPin(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    try {
      await fetch("/api/platform/v1/auth/recover/request", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email: recoveryEmail }),
      });
    } catch {
      // ConnectionMonitor surfaces; values preserved.
    }
  }

  async function handleSubmitNewPassword(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!canSubmitNewPassword) return;
    try {
      await fetch("/api/platform/v1/auth/recover/complete", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ pin: pinValue, password: newPassword }),
      });
    } catch {
      // ConnectionMonitor surfaces.
    }
  }
</script>

<svelte:head>
  <title>Recover password · {branding.appName} Admin</title>
</svelte:head>

<section class="flex w-full max-w-2xl flex-col gap-6">
  <header class="flex flex-col gap-2">
    <h1 class="text-2xl font-semibold">Recover your password</h1>
    <p class="text-sm text-muted-foreground">
      We'll write a recovery PIN to a local file you can read. Three quick steps.
    </p>
  </header>

  <WizardProgress
    steps={STEPS}
    currentId={currentStep}
    testId="recover-progress"
    stepTestidPrefix="recover-step"
    onSelect={selectStep}
  />

  <Card>
    <CardContent>
      {#if currentStep === "request-pin"}
        <form class="flex flex-col gap-4" onsubmit={handleRequestPin}>
          <div class="grid gap-2">
            <Label for="recover-email">Email</Label>
            <Input
              id="recover-email"
              type="email"
              autocomplete="email"
              required
              bind:value={recoveryEmail}
            />
          </div>
          <Button type="submit" class="self-start">Send PIN</Button>
        </form>
      {:else if currentStep === "enter-pin"}
        <form class="flex flex-col gap-4" onsubmit={(e) => e.preventDefault()}>
          <div class="grid gap-2">
            <Label for="recover-pin">Recovery PIN</Label>
            <Input
              id="recover-pin"
              type="text"
              inputmode="numeric"
              bind:value={pinValue}
            />
          </div>
          <Button type="submit" class="self-start">Continue</Button>
        </form>
      {:else if currentStep === "new-password"}
        <form class="flex flex-col gap-4" onsubmit={handleSubmitNewPassword}>
          <div class="grid gap-2">
            <Label for="recover-new-password">New password</Label>
            <Input
              id="recover-new-password"
              type="password"
              autocomplete="new-password"
              bind:value={newPassword}
            />
            <CardDescription>
              At least 12 characters, including at least one number.
            </CardDescription>
          </div>
          {#if strengthError}
            <p data-testid="password-strength-error" class="text-sm text-destructive">
              {strengthError}
            </p>
          {/if}
          <Button
            type="submit"
            disabled={!canSubmitNewPassword}
            class="self-start"
          >
            Set password
          </Button>
        </form>
      {/if}
    </CardContent>
  </Card>
</section>
