<script lang="ts">
  import WizardProgress, { type WizardStep } from "$lib/components/WizardProgress.svelte";

  let { data } = $props();
  let branding = $derived(data.branding);

  type StepId = "request-pin" | "enter-pin" | "new-password";

  const STEPS: WizardStep[] = [
    { id: "request-pin", label: "Request PIN" },
    { id: "enter-pin", label: "Enter PIN" },
    { id: "new-password", label: "New password" },
  ];

  let currentStep = $state<StepId>("request-pin");

  // Step 1
  let recoveryEmail = $state("");

  // Step 2
  let pinValue = $state("");

  // Step 3
  let newPassword = $state("");

  // Strength rules: minimum length + at least one digit. Surfaced as a single
  // composite rule for now; PR 2B can split into per-rule indicators.
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

  <div class="rounded border border-border bg-background p-6 shadow-sm">
    {#if currentStep === "request-pin"}
      <form class="flex flex-col gap-4" onsubmit={handleRequestPin}>
        <label class="flex flex-col gap-1">
          <span class="text-sm font-medium">Email</span>
          <input
            type="email"
            autocomplete="email"
            required
            bind:value={recoveryEmail}
            class="rounded border border-border px-3 py-2 text-sm"
          />
        </label>
        <button
          type="submit"
          class="self-start rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground"
        >
          Send PIN
        </button>
      </form>
    {:else if currentStep === "enter-pin"}
      <form class="flex flex-col gap-4" onsubmit={(e) => e.preventDefault()}>
        <label class="flex flex-col gap-1">
          <span class="text-sm font-medium">Recovery PIN</span>
          <input
            type="text"
            inputmode="numeric"
            bind:value={pinValue}
            class="rounded border border-border px-3 py-2 text-sm"
          />
        </label>
        <button
          type="submit"
          class="self-start rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground"
        >
          Continue
        </button>
      </form>
    {:else if currentStep === "new-password"}
      <form class="flex flex-col gap-4" onsubmit={handleSubmitNewPassword}>
        <label class="flex flex-col gap-1">
          <span class="text-sm font-medium">New password</span>
          <input
            type="password"
            autocomplete="new-password"
            bind:value={newPassword}
            class="rounded border border-border px-3 py-2 text-sm"
          />
          <span class="text-xs text-muted-foreground">
            At least 12 characters, including at least one number.
          </span>
        </label>
        {#if strengthError}
          <p data-testid="password-strength-error" class="text-sm text-destructive">
            {strengthError}
          </p>
        {/if}
        <button
          type="submit"
          disabled={!canSubmitNewPassword}
          class="self-start rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50"
        >
          Set password
        </button>
      </form>
    {/if}
  </div>
</section>
