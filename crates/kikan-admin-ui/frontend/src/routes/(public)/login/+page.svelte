<script lang="ts">
  import { base } from "$app/paths";

  let { data } = $props();

  let email = $state("");
  let password = $state("");
  let submitting = $state(false);
  let branding = $derived(data.branding);
  let showFirstTimeSetup = $derived(
    data.setupStatus !== undefined && !data.setupStatus.admin_exists,
  );

  async function handleSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    submitting = true;
    try {
      // PR 2B wires the actual sign-in call. PR 2A only proves the form
      // posts somewhere — the offline scenario asserts a self-healing
      // banner appears, which the fetch failure path drives.
      await fetch("/api/platform/v1/auth/sign-in", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email, password }),
      });
    } catch {
      // ConnectionMonitor in the public layout owns the banner; the form
      // simply preserves its values for the user to retry.
    } finally {
      submitting = false;
    }
  }
</script>

<svelte:head>
  <title>Sign in · {branding.appName} Admin</title>
</svelte:head>

<form
  data-testid="sign-in-form"
  class="flex w-full max-w-sm flex-col gap-4 rounded border border-border bg-background p-6 shadow-sm"
  onsubmit={handleSubmit}
>
  <h1 class="text-xl font-semibold">Sign in</h1>
  <p class="text-sm text-muted-foreground">
    Sign in to manage your {branding.shopNounSingular}.
  </p>

  <label class="flex flex-col gap-1">
    <span class="text-sm font-medium">Email</span>
    <input
      type="email"
      autocomplete="email"
      required
      bind:value={email}
      class="rounded border border-border px-3 py-2 text-sm"
    />
  </label>

  <label class="flex flex-col gap-1">
    <span class="text-sm font-medium">Password</span>
    <input
      type="password"
      autocomplete="current-password"
      required
      bind:value={password}
      class="rounded border border-border px-3 py-2 text-sm"
    />
  </label>

  <button
    type="submit"
    disabled={submitting}
    class="rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50"
  >
    Sign in
  </button>

  <div class="flex items-center justify-between text-sm">
    <a href="{base}/recover" class="text-primary underline">Forgot password?</a>
    {#if showFirstTimeSetup}
      <a data-testid="first-time-setup-link" href="{base}/setup" class="text-primary underline">
        First time setup?
      </a>
    {/if}
  </div>
</form>
