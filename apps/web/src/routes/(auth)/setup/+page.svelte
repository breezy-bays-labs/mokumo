<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { untrack } from "svelte";
  import { apiFetch } from "$lib/api";
  import CopyableUrl from "$lib/components/copyable-url.svelte";
  import PasswordInput from "$lib/components/password-input.svelte";
  import RecoveryCodes from "$lib/components/recovery-codes.svelte";
  import { Button } from "$lib/components/ui/button";
  import {
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import { Alert, AlertDescription } from "$lib/components/ui/alert";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import type { ServerInfoResponse } from "$lib/types/ServerInfoResponse";

  let step = $state(1);
  let error = $state<string | null>(null);
  let loading = $state(false);

  // Step 2: Shop info
  let shopName = $state("");
  let slug = $derived(
    shopName
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, ""),
  );

  // Step 3: Admin account
  let adminName = $state("");
  let adminEmail = $state("");
  let adminPassword = $state("");
  let setupToken = $state(page.url.searchParams.get("setup_token") ?? "");

  // Step 3: Token visibility — hide only when URL param has a non-empty value
  let showTokenField = $state(!page.url.searchParams.get("setup_token"));

  // Step 4: Recovery codes
  let recoveryCodes = $state<string[]>([]);
  let codesSaved = $state(false);

  // Completion screen: fetch server info for LAN URL display
  let completionServerInfo = $state<ServerInfoResponse | null>(null);
  let completionDisplayUrl = $derived(
    completionServerInfo?.lan_url ?? completionServerInfo?.ip_url ?? null,
  );

  $effect(() => {
    if (step === 5 && !untrack(() => completionServerInfo)) {
      apiFetch<ServerInfoResponse>("/api/server-info").then((result) => {
        if (result.ok && "data" in result) {
          completionServerInfo = result.data;
        } else {
          console.error("Failed to fetch server info for completion screen");
        }
      });
    }
  });

  let step2Valid = $derived(shopName.trim().length > 0);
  let step3Valid = $derived(
    adminName.trim().length > 0 &&
      adminEmail.length > 0 &&
      adminPassword.length >= 8 &&
      setupToken.trim().length > 0,
  );

  async function handleCreateAccount() {
    error = null;
    loading = true;

    const result = await apiFetch<{ recovery_codes: string[] }>("/api/setup", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        shop_name: shopName,
        admin_name: adminName,
        admin_email: adminEmail,
        admin_password: adminPassword,
        setup_token: setupToken,
      }),
    });

    if (!result.ok) {
      error = result.error.message;
      showTokenField = true;
      loading = false;
      return;
    }

    if ("data" in result) {
      recoveryCodes = result.data.recovery_codes ?? [];
    }
    step = 4;
    loading = false;
  }
</script>

{#if step === 1}
  <Card>
    <CardHeader class="text-center">
      <CardTitle class="text-2xl">Welcome to Mokumo Print</CardTitle>
      <CardDescription>
        Let's get your shop set up. This will only take a minute.
      </CardDescription>
    </CardHeader>
    <CardContent>
      <Button class="w-full" onclick={() => (step = 2)}>
        Get Started
        <ArrowRight class="ml-2 h-4 w-4" />
      </Button>
    </CardContent>
  </Card>
{:else if step === 2}
  <Card>
    <CardHeader>
      <CardTitle>Shop Information</CardTitle>
      <CardDescription>What's your shop called?</CardDescription>
    </CardHeader>
    <CardContent>
      <form
        class="space-y-4"
        onsubmit={(e) => {
          e.preventDefault();
          step = 3;
        }}
      >
        <div class="space-y-2">
          <Label for="shop-name">Shop name</Label>
          <Input
            id="shop-name"
            placeholder="My Print Shop"
            bind:value={shopName}
            required
          />
        </div>

        {#if slug}
          <p class="text-sm text-muted-foreground">
            Your shop will be available at
            <span class="font-mono text-foreground">{slug}.local</span>
          </p>
        {/if}

        <Button type="submit" class="w-full" disabled={!step2Valid}>
          Continue
          <ArrowRight class="ml-2 h-4 w-4" />
        </Button>
      </form>
    </CardContent>
  </Card>
{:else if step === 3}
  <Card>
    <CardHeader>
      <CardTitle>Admin Account</CardTitle>
      <CardDescription>Create your administrator account.</CardDescription>
    </CardHeader>
    <CardContent>
      {#if error}
        <Alert variant="destructive" class="mb-4">
          <CircleAlert class="h-4 w-4" />
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      {/if}

      <form
        class="space-y-4"
        onsubmit={(e) => {
          e.preventDefault();
          handleCreateAccount();
        }}
      >
        <div class="space-y-2">
          <Label for="admin-name">Name</Label>
          <Input
            id="admin-name"
            placeholder="Your name"
            bind:value={adminName}
            autocomplete="name"
            required
          />
        </div>

        <div class="space-y-2">
          <Label for="admin-email">Email</Label>
          <Input
            id="admin-email"
            type="email"
            placeholder="you@example.com"
            bind:value={adminEmail}
            autocomplete="email"
            required
          />
        </div>

        <div class="space-y-2">
          <Label for="admin-password">Password</Label>
          <PasswordInput
            id="admin-password"
            bind:value={adminPassword}
            showStrength
          />
        </div>

        {#if showTokenField}
          <div class="space-y-2">
            <Label for="setup-token">Setup token</Label>
            <Input
              id="setup-token"
              type="text"
              placeholder="From terminal output"
              bind:value={setupToken}
              class="font-mono"
              autocomplete="off"
            />
            <p class="text-xs text-muted-foreground">
              {page.url.searchParams.has("setup_token")
                ? "Prefilled by the desktop app for first-run setup."
                : "Find this in the terminal where you started Mokumo."}
            </p>
          </div>
        {/if}

        <Button type="submit" class="w-full" disabled={!step3Valid || loading}>
          {loading ? "Creating account..." : "Create Account"}
        </Button>
      </form>
    </CardContent>
  </Card>
{:else if step === 4}
  <Card>
    <CardHeader>
      <CardTitle>Recovery Codes</CardTitle>
      <CardDescription>
        Save these codes somewhere safe. You'll need them if you lose access to
        your account.
      </CardDescription>
    </CardHeader>
    <CardContent class="space-y-4">
      <RecoveryCodes codes={recoveryCodes} />

      <div class="flex items-center gap-2">
        <Checkbox
          id="codes-saved"
          checked={codesSaved}
          onCheckedChange={(checked) => {
            if (typeof checked === "boolean") codesSaved = checked;
          }}
        />
        <Label for="codes-saved" class="text-sm font-normal">
          I have saved my recovery codes
        </Label>
      </div>

      <Button class="w-full" disabled={!codesSaved} onclick={() => (step = 5)}>
        Continue
        <ArrowRight class="ml-2 h-4 w-4" />
      </Button>
    </CardContent>
  </Card>
{:else if step === 5}
  <Card>
    <CardHeader class="text-center">
      <div
        class="mx-auto mb-2 flex h-12 w-12 items-center justify-center rounded-full bg-green-100 dark:bg-green-900/30"
      >
        <Check class="h-6 w-6 text-green-600 dark:text-green-400" />
      </div>
      <CardTitle class="text-2xl">You're all set!</CardTitle>
      <CardDescription>
        Your shop <span class="font-semibold text-foreground">{shopName}</span> is
        ready.
      </CardDescription>
    </CardHeader>
    <CardContent>
      {#if completionDisplayUrl}
        <div class="space-y-2 mb-4">
          <p class="text-sm text-muted-foreground">
            Other devices on your network can reach your shop at:
          </p>
          <CopyableUrl
            url={completionDisplayUrl}
            label="Copy LAN URL to clipboard"
            testId="copy-setup-lan-url"
          />
        </div>
      {/if}
      <Button class="w-full" onclick={() => goto("/")}>
        Open Dashboard
        <ArrowRight class="ml-2 h-4 w-4" />
      </Button>
    </CardContent>
  </Card>
{/if}
