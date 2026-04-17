<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { apiFetch } from "$lib/api";
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
  import CheckCircle from "@lucide/svelte/icons/check-circle";
  import X from "@lucide/svelte/icons/x";

  let restored = $derived(page.url.searchParams.get("restored") === "true");
  let bannerDismissed = $state(false);

  let email = $state("");
  let password = $state("");
  let error = $state<string | null>(null);
  let loading = $state(false);

  let isValid = $derived(email.length > 0 && password.length > 0);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    error = null;
    loading = true;

    const result = await apiFetch("/api/auth/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email, password }),
    });
    loading = false;

    if (!result.ok) {
      error = result.error.message;
      return;
    }

    goto("/");
  }
</script>

<Card>
  <CardHeader>
    <CardTitle>Sign in</CardTitle>
    <CardDescription
      >Enter your credentials to access your shop.</CardDescription
    >
  </CardHeader>
  <CardContent>
    {#if restored && !bannerDismissed}
      <Alert class="mb-4 flex items-start gap-2">
        <CheckCircle class="h-4 w-4 shrink-0 mt-0.5" />
        <AlertDescription class="flex-1">
          Your shop data has been imported. Sign in with your existing
          credentials.
        </AlertDescription>
        <Button
          variant="ghost"
          size="icon"
          class="h-6 w-6 shrink-0 text-muted-foreground hover:text-foreground"
          aria-label="Dismiss"
          data-testid="dismiss-restore-banner"
          onclick={() => (bannerDismissed = true)}
        >
          <X class="h-4 w-4" />
        </Button>
      </Alert>
    {/if}

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
        <Label for="password">Password</Label>
        <Input
          id="password"
          type="password"
          placeholder="Password"
          bind:value={password}
          autocomplete="current-password"
          required
        />
      </div>

      <Button type="submit" class="w-full" disabled={!isValid || loading}>
        {loading ? "Signing in..." : "Sign in"}
      </Button>
    </form>

    <div class="mt-4 flex justify-between text-sm">
      <a
        href="/forgot-password"
        class="text-muted-foreground hover:text-foreground underline-offset-4 hover:underline"
      >
        Forgot password?
      </a>
      <a
        href="/recovery"
        class="text-muted-foreground hover:text-foreground underline-offset-4 hover:underline"
      >
        Use recovery code
      </a>
    </div>
  </CardContent>
</Card>
