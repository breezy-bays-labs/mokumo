<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { apiFetch } from "$lib/api";
  import { toast } from "$lib/components/toast";
  import Spinner from "$lib/components/spinner.svelte";
  import { Button } from "$lib/components/ui/button";
  import type { SetupStatusResponse } from "$lib/types/SetupStatusResponse";
  import type { ProfileSwitchRequest } from "$lib/types/ProfileSwitchRequest";
  import type { ProfileSwitchResponse } from "$lib/types/ProfileSwitchResponse";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";

  const MAX_RETRIES = 10;
  const RETRY_DELAY_MS = 500;

  type PageState =
    | { kind: "starting" }
    | { kind: "ready"; productionSetupComplete: boolean }
    | { kind: "error" };

  let pageState = $state<PageState>({ kind: "starting" });
  let switching = $state(false);
  let navigating = $state(false);
  /** Guard against concurrent checkStatus invocations. */
  let checking = false;

  async function fetchSetupStatus(): Promise<SetupStatusResponse | null> {
    for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
      const result = await apiFetch<SetupStatusResponse>("/api/setup-status");
      if (result.ok && "data" in result) {
        return result.data;
      }
      const detail = result.ok
        ? "unexpected ok with no data"
        : `status=${result.status} code=${result.error.code} msg="${result.error.message}"`;
      console.warn(
        `[welcome] setup-status attempt ${attempt + 1}/${MAX_RETRIES} failed: ${detail}`,
      );
      if (attempt < MAX_RETRIES - 1) {
        await new Promise((resolve) => setTimeout(resolve, RETRY_DELAY_MS));
      }
    }
    console.error(
      "[welcome] setup-status exhausted all retries — showing error state",
    );
    return null;
  }

  async function checkStatus() {
    if (checking) return;
    checking = true;
    try {
      // Only show the loading spinner on initial load, not on tab-focus re-checks
      // to avoid flicker when the user is already seeing the ready state.
      if (pageState.kind !== "ready") {
        pageState = { kind: "starting" };
      }
      const status = await fetchSetupStatus();
      if (!status) {
        pageState = { kind: "error" };
        return;
      }
      // Another session already completed onboarding — leave the welcome screen.
      if (!status.is_first_launch) {
        try {
          await goto("/");
        } catch (err) {
          console.error(
            "[welcome] goto('/') failed after is_first_launch=false:",
            err,
          );
          pageState = { kind: "error" };
        }
        return;
      }
      pageState = {
        kind: "ready",
        productionSetupComplete: status.production_setup_complete,
      };
    } finally {
      checking = false;
    }
  }

  async function handleSwitch(target: ProfileSwitchRequest["profile"]) {
    switching = true;
    const result = await apiFetch<ProfileSwitchResponse>(
      "/api/profile/switch",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          profile: target,
        } satisfies ProfileSwitchRequest),
      },
    );

    if (!result.ok) {
      // Only reset switching on error so buttons stay disabled until navigation completes.
      switching = false;
      if (result.status === 429) {
        toast.error("Too many attempts — please wait a moment.");
      } else if (result.status === 503) {
        // Production not yet set up — go to setup wizard.
        try {
          await goto("/setup");
        } catch (err) {
          console.error("[welcome] goto('/setup') failed after 503:", err);
        }
      } else {
        const msg =
          result.error.code === "network_error"
            ? "Could not reach the server. Check your connection and try again."
            : result.error.code === "parse_error"
              ? "Received an unexpected response from the server."
              : result.error.message;
        toast.error(msg);
      }
      return;
    }

    // Keep switching=true — component will unmount on successful navigation.
    try {
      await goto("/");
    } catch (err) {
      switching = false;
      console.error(
        "[welcome] goto('/') failed after successful profile switch:",
        err,
      );
      toast.error("Navigation failed. Please refresh the page.");
    }
  }

  onMount(() => {
    checkStatus();

    // Re-check setup-status when the tab becomes visible (stale-tab guard).
    // Do not re-check while a switch is in flight to avoid racing with handleSwitch.
    const handler = () => {
      if (document.visibilityState === "visible" && !switching) {
        checkStatus();
      }
    };
    document.addEventListener("visibilitychange", handler);
    return () => document.removeEventListener("visibilitychange", handler);
  });
</script>

{#if pageState.kind === "starting"}
  <div
    class="flex flex-col items-center gap-3 text-center"
    data-testid="startup-message"
  >
    <Spinner size="lg" />
    <p class="text-sm text-muted-foreground">Starting up...</p>
    <p class="text-xs text-muted-foreground">
      Mokumo is getting ready. This only takes a moment.
    </p>
  </div>
{:else if pageState.kind === "error"}
  <div
    class="flex flex-col items-center gap-4 text-center"
    data-testid="error-state"
  >
    <p class="text-sm font-medium">Could not reach Mokumo</p>
    <p class="text-xs text-muted-foreground">
      The server didn't respond after several attempts.
    </p>
    <Button
      variant="outline"
      size="sm"
      onclick={() => checkStatus()}
      data-testid="refresh-button"
    >
      <RefreshCw class="mr-2 h-4 w-4" />
      Refresh
    </Button>
  </div>
{:else}
  <div class="flex flex-col gap-6">
    <div class="text-center">
      <h1 class="text-xl font-semibold">Welcome to Mokumo</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        How would you like to get started?
      </p>
    </div>

    <div class="flex flex-col gap-3">
      <Button
        class="w-full"
        disabled={switching || navigating}
        onclick={() => handleSwitch("production")}
        data-testid="setup-shop-button"
      >
        {#if switching}
          <Spinner size="sm" class="mr-2" />
          Switching...
        {:else}
          Set Up My Shop
        {/if}
      </Button>

      <Button
        variant="outline"
        class="w-full"
        disabled={switching || navigating}
        onclick={() => handleSwitch("demo")}
        data-testid="explore-demo-button"
      >
        {#if switching}
          <Spinner size="sm" class="mr-2" />
          Switching...
        {:else}
          Explore Demo
        {/if}
      </Button>

      <Button
        variant="outline"
        class="w-full"
        disabled={switching || navigating}
        onclick={async () => {
          navigating = true;
          try {
            await goto("/welcome/restore", { state: { fromWelcome: true } });
          } catch {
            navigating = false;
          }
        }}
        data-testid="open-existing-shop-button"
      >
        Open Existing Shop
      </Button>
    </div>
  </div>
{/if}
