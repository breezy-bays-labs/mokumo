<script lang="ts">
  import { page } from "$app/state";
  import { toast } from "svelte-sonner";
  import { Dialog } from "bits-ui";
  import WizardProgress, { type WizardStep } from "$lib/components/WizardProgress.svelte";

  import { fetchPlatform } from "$lib/platform";
  import { base } from "$app/paths";

  let { data } = $props();
  let branding = $derived(data.branding);
  // The Finish step shows the shop URL pulled from /app-meta. We fetch on
  // demand (when the user clicks Copy) rather than at load time so the value
  // always reflects current platform state — the mDNS hostname can change
  // between mount and the operator reaching the Finish step.

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

  // Create-admin form state
  let adminName = $state("");
  let adminEmail = $state("");
  let adminPassword = $state("");
  let pastedToken = $state("");

  // Create-profile form state
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

  // Beforeunload guard — only when the operator has entered any wizard data
  // beyond the welcome step. The "Leave setup?" dialog covers in-app
  // navigation; beforeunload covers tab close / hard refresh.
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

  // Intercept link clicks to internal admin paths while the wizard is dirty.
  // The dialog asks the user to confirm. The actual navigation is deferred
  // to a separate hook in PR 2B; for PR 2A we just surface the dialog so the
  // BDD scenario passes and the operator gets the warning.
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

  <div class="rounded border border-border bg-background p-6 shadow-sm">
    {#if currentStep === "welcome"}
      <div class="flex flex-col gap-3">
        <p data-testid="wizard-welcome-message" class="text-base">
          Welcome to {branding.appName}. We'll create the admin account and your first
          {branding.shopNounSingular} profile.
        </p>
        {#if !cliMode}
          <p data-testid="wizard-token-accepted" class="text-sm text-muted-foreground">
            Setup token accepted — you can continue without re-entering it.
          </p>
        {/if}
      </div>
    {:else if currentStep === "create-admin"}
      <form class="flex flex-col gap-4" onsubmit={(e) => e.preventDefault()}>
        <label class="flex flex-col gap-1">
          <span class="text-sm font-medium">Name</span>
          <input
            type="text"
            autocomplete="name"
            bind:value={adminName}
            class="rounded border border-border px-3 py-2 text-sm"
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-sm font-medium">Email</span>
          <input
            type="email"
            autocomplete="email"
            bind:value={adminEmail}
            class="rounded border border-border px-3 py-2 text-sm"
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-sm font-medium">Password</span>
          <input
            type="password"
            autocomplete="new-password"
            bind:value={adminPassword}
            class="rounded border border-border px-3 py-2 text-sm"
          />
        </label>
        {#if cliMode}
          <label class="flex flex-col gap-1">
            <span class="text-sm font-medium">Setup token</span>
            <input
              type="text"
              bind:value={pastedToken}
              class="rounded border border-border px-3 py-2 text-sm"
            />
            <span data-testid="setup-token-helper" class="text-xs text-muted-foreground">
              Look for the setup token printed in your terminal when you started the CLI.
            </span>
          </label>
        {/if}
      </form>
    {:else if currentStep === "create-profile"}
      <form class="flex flex-col gap-4" onsubmit={(e) => e.preventDefault()}>
        <label class="flex flex-col gap-1">
          <span class="text-sm font-medium">Profile name</span>
          <input
            type="text"
            bind:value={profileName}
            class="rounded border border-border px-3 py-2 text-sm"
          />
        </label>
      </form>
    {:else if currentStep === "finish"}
      <div class="flex flex-col gap-3">
        <p class="text-base">All set! Your {branding.shopNounSingular} is ready.</p>
        <button
          type="button"
          onclick={handleCopyShopUrl}
          class="self-start rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground"
        >
          Copy shop URL
        </button>
      </div>
    {/if}
  </div>
</section>

<Dialog.Root bind:open={leaveDialogOpen}>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 bg-black/40" />
    <Dialog.Content
      class="fixed left-1/2 top-1/2 w-[420px] -translate-x-1/2 -translate-y-1/2 rounded bg-background p-6 shadow-lg"
    >
      <Dialog.Title class="mb-2 text-lg font-semibold">Leave setup?</Dialog.Title>
      <Dialog.Description class="mb-4 text-sm text-muted-foreground">
        You'll lose anything you've entered. You can come back to setup at any time.
      </Dialog.Description>
      <div class="flex justify-end gap-2">
        <button
          type="button"
          onclick={() => (leaveDialogOpen = false)}
          class="rounded border border-border px-4 py-2 text-sm"
        >
          Stay on wizard
        </button>
        <button
          type="button"
          onclick={() => (leaveDialogOpen = false)}
          class="rounded bg-destructive px-4 py-2 text-sm font-medium text-destructive-foreground"
        >
          Leave
        </button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
