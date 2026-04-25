<script lang="ts">
  import { page } from "$app/state";
  import { toast } from "svelte-sonner";
  import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
  } from "$lib/components/ui/dialog/index.js";
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
  import WizardProgress, { type WizardStep } from "$lib/components/WizardProgress.svelte";

  import { fetchPlatform } from "$lib/platform";
  import { base } from "$app/paths";

  let { data } = $props();
  let branding = $derived(data.branding);

  type StepId = "welcome" | "create-admin" | "create-profile" | "finish";

  const STEPS: WizardStep[] = [
    { id: "welcome", label: "Welcome" },
    { id: "create-admin", label: "Create admin" },
    { id: "create-profile", label: "Create profile" },
    { id: "finish", label: "Finish" },
  ];

  let currentStep = $state<StepId>("welcome");
  let setupToken = $derived(page.url.searchParams.get("setup_token") ?? "");
  let cliMode = $derived(setupToken === "");

  let adminName = $state("");
  let adminEmail = $state("");
  let adminPassword = $state("");
  let pastedToken = $state("");

  let profileName = $state("");

  let leaveDialogOpen = $state(false);

  function selectStep(id: string): void {
    currentStep = id as StepId;
  }

  async function handleCopyShopUrl(): Promise<void> {
    try {
      const meta = await fetchPlatform<{ mdns_hostname: string | null; port: number | null }>(
        "/app-meta",
      );
      if (!meta.mdns_hostname || meta.port === null) return;
      const url = `http://${meta.mdns_hostname}:${meta.port}`;
      await navigator.clipboard.writeText(url);
      toast.success("URL copied to clipboard");
    } catch {
      // Network failure — ConnectionMonitor's banner handles surfacing.
    }
  }

  let dirty = $derived(
    currentStep !== "welcome" &&
      (adminName !== "" || adminEmail !== "" || adminPassword !== "" || profileName !== ""),
  );

  $effect(() => {
    function onBeforeUnload(event: BeforeUnloadEvent): void {
      if (!dirty) return;
      event.preventDefault();
      event.returnValue = "";
    }
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  });

  $effect(() => {
    function onClick(e: MouseEvent): void {
      if (currentStep === "welcome") return;
      const target = e.target as HTMLElement | null;
      const anchor = target?.closest("a[href]") as HTMLAnchorElement | null;
      if (!anchor) return;
      const href = anchor.getAttribute("href") ?? "";
      if (!href.startsWith("/admin")) return;
      if (anchor.dataset.bypassLeaveGuard === "true") return;
      e.preventDefault();
      leaveDialogOpen = true;
    }
    document.addEventListener("click", onClick, true);
    return () => document.removeEventListener("click", onClick, true);
  });
</script>

<svelte:head>
  <title>Set up · {branding.appName} Admin</title>
</svelte:head>

<section class="flex w-full max-w-2xl flex-col gap-6">
  <header class="flex items-start justify-between gap-2">
    <div class="flex flex-col gap-2">
      <h1 class="text-2xl font-semibold">Set up your {branding.shopNounSingular}</h1>
      <p class="text-sm text-muted-foreground">
        Four quick steps and you'll be ready to go.
      </p>
    </div>
    <a
      data-testid="wizard-cancel-link"
      href="{base}/login"
      class="text-sm text-muted-foreground underline"
    >
      Back to sign-in
    </a>
  </header>

  <WizardProgress
    steps={STEPS}
    currentId={currentStep}
    testId="wizard-progress"
    stepTestidPrefix="wizard-step"
    onSelect={selectStep}
  />

  <Card>
    {#if currentStep === "welcome"}
      <CardHeader>
        <CardTitle>Welcome</CardTitle>
      </CardHeader>
      <CardContent class="flex flex-col gap-3">
        <p data-testid="wizard-welcome-message" class="text-base">
          Welcome to {branding.appName}. We'll create the admin account and your first
          {branding.shopNounSingular} profile.
        </p>
        {#if !cliMode}
          <p data-testid="wizard-token-accepted" class="text-sm text-muted-foreground">
            Setup token accepted — you can continue without re-entering it.
          </p>
        {/if}
      </CardContent>
    {:else if currentStep === "create-admin"}
      <CardHeader>
        <CardTitle>Create admin</CardTitle>
        <CardDescription>This account manages your {branding.shopNounSingular}.</CardDescription>
      </CardHeader>
      <CardContent>
        <form class="flex flex-col gap-4" onsubmit={(e) => e.preventDefault()}>
          <div class="grid gap-2">
            <Label for="setup-admin-name">Name</Label>
            <Input id="setup-admin-name" type="text" autocomplete="name" bind:value={adminName} />
          </div>
          <div class="grid gap-2">
            <Label for="setup-admin-email">Email</Label>
            <Input
              id="setup-admin-email"
              type="email"
              autocomplete="email"
              bind:value={adminEmail}
            />
          </div>
          <div class="grid gap-2">
            <Label for="setup-admin-password">Password</Label>
            <Input
              id="setup-admin-password"
              type="password"
              autocomplete="new-password"
              bind:value={adminPassword}
            />
          </div>
          {#if cliMode}
            <div class="grid gap-2">
              <Label for="setup-token">Setup token</Label>
              <Input id="setup-token" type="text" bind:value={pastedToken} />
              <p data-testid="setup-token-helper" class="text-xs text-muted-foreground">
                Look for the setup token printed in your terminal when you started the CLI.
              </p>
            </div>
          {/if}
        </form>
      </CardContent>
    {:else if currentStep === "create-profile"}
      <CardHeader>
        <CardTitle>Create profile</CardTitle>
      </CardHeader>
      <CardContent>
        <form class="flex flex-col gap-4" onsubmit={(e) => e.preventDefault()}>
          <div class="grid gap-2">
            <Label for="setup-profile-name">Profile name</Label>
            <Input id="setup-profile-name" type="text" bind:value={profileName} />
          </div>
        </form>
      </CardContent>
    {:else if currentStep === "finish"}
      <CardHeader>
        <CardTitle>All set</CardTitle>
      </CardHeader>
      <CardContent class="flex flex-col gap-3">
        <p class="text-base">Your {branding.shopNounSingular} is ready.</p>
        <Button type="button" onclick={handleCopyShopUrl} class="self-start">
          Copy shop URL
        </Button>
      </CardContent>
    {/if}
  </Card>
</section>

<Dialog bind:open={leaveDialogOpen}>
  <DialogContent class="sm:max-w-md">
    <DialogHeader>
      <DialogTitle>Leave setup?</DialogTitle>
      <DialogDescription>
        You'll lose anything you've entered. You can come back to setup at any time.
      </DialogDescription>
    </DialogHeader>
    <DialogFooter>
      <Button type="button" variant="outline" onclick={() => (leaveDialogOpen = false)}>
        Stay on wizard
      </Button>
      <Button type="button" variant="destructive" onclick={() => (leaveDialogOpen = false)}>
        Leave
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
