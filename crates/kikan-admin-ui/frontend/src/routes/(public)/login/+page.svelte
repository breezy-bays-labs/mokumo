<script lang="ts">
  import { base } from "$app/paths";
  import {
    Card,
    CardContent,
    CardDescription,
    CardFooter,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card/index.js";
  import { Button } from "$lib/components/ui/button/index.js";
  import { Input } from "$lib/components/ui/input/index.js";
  import { Label } from "$lib/components/ui/label/index.js";

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

<Card class="w-full max-w-sm">
  <CardHeader>
    <CardTitle>Sign in</CardTitle>
    <CardDescription>Sign in to manage your {branding.shopNounSingular}.</CardDescription>
  </CardHeader>
  <CardContent>
    <form data-testid="sign-in-form" class="flex flex-col gap-4" onsubmit={handleSubmit}>
      <div class="grid gap-2">
        <Label for="sign-in-email">Email</Label>
        <Input
          id="sign-in-email"
          type="email"
          autocomplete="email"
          required
          bind:value={email}
        />
      </div>
      <div class="grid gap-2">
        <Label for="sign-in-password">Password</Label>
        <Input
          id="sign-in-password"
          type="password"
          autocomplete="current-password"
          required
          bind:value={password}
        />
      </div>
      <Button type="submit" disabled={submitting}>Sign in</Button>
    </form>
  </CardContent>
  <CardFooter class="flex justify-between text-sm">
    <a href="{base}/recover" class="text-primary underline">Forgot password?</a>
    {#if showFirstTimeSetup}
      <a data-testid="first-time-setup-link" href="{base}/setup" class="text-primary underline">
        First time setup?
      </a>
    {/if}
  </CardFooter>
</Card>
