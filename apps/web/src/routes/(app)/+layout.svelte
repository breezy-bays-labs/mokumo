<script lang="ts">
  import type { Snippet } from "svelte";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import AppSidebar from "$lib/components/app-sidebar.svelte";
  import AppTopbar from "$lib/components/app-topbar.svelte";
  import DemoBanner from "$lib/components/demo-banner.svelte";
  import DisconnectBanner from "$lib/components/disconnect-banner.svelte";
  import RecoveryCodeWarning from "$lib/components/recovery-code-warning.svelte";
  import UnsavedChangesDialog from "$lib/components/unsaved-changes-dialog.svelte";
  import { SidebarInset, SidebarProvider } from "$lib/components/ui/sidebar";
  import { apiFetch } from "$lib/api";
  import {
    installNavigationGuard,
    replayNavigation,
  } from "$lib/navigation-guard";
  import { profile } from "$lib/stores/profile.svelte";
  import {
    markConnected,
    markDisconnected,
    markShutdown,
  } from "$lib/stores/ws-status.svelte";
  import { createWebSocketConnection } from "$lib/ws";
  import { toast } from "$lib/components/toast";
  import type { LayoutData } from "./$types";

  let { children, data }: { children: Snippet; data: LayoutData } = $props();

  const STORAGE_KEY = "sidebar:state";

  let sidebarOpen = $state(
    typeof window !== "undefined"
      ? localStorage.getItem(STORAGE_KEY) !== "false"
      : true,
  );

  function handleOpenChange(open: boolean) {
    sidebarOpen = open;
    localStorage.setItem(STORAGE_KEY, String(open));
  }

  installNavigationGuard();

  onMount(() => {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/ws`;
    const ws = createWebSocketConnection(wsUrl, {
      onMessage: () => {},
      onReconnect: markConnected,
      onDisconnect: markDisconnected,
      onShutdown: markShutdown,
    });

    // Expose store helpers for Playwright BDD tests
    (window as any).__wsStatusTestHelpers = {
      markConnected,
      markDisconnected,
      markShutdown,
    };

    return () => ws.close();
  });

  let confirmSwitching = $state(false);

  let dialogDescription = $derived(
    profile.switchTarget
      ? "You have unsaved changes that will be lost if you switch profiles."
      : "You have unsaved changes that will be lost if you leave this page.",
  );

  async function handleDirtyConfirm() {
    // Profile-switch context
    if (profile.switchTarget) {
      if (confirmSwitching) return;
      const target = profile.switchTarget;
      confirmSwitching = true;
      try {
        const result = await apiFetch("/api/profile/switch", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ profile: target }),
        });
        if (!result.ok) {
          console.error(
            "Profile switch failed (dirty path):",
            result.status,
            result.error,
          );
          toast.error("Failed to switch profile. Please try again.");
          return;
        }
        profile.unsavedChangesDialogOpen = false;
        profile.dirtyForms.clear();
        profile.switchTarget = null;
        try {
          await goto("/");
        } catch (error) {
          console.error("Profile switch navigation failed:", error);
          window.location.assign("/");
        }
      } finally {
        confirmSwitching = false;
      }
      return;
    }

    // Navigation context — replay with history.go() for back/forward,
    // goto() for link clicks, to preserve browser history semantics.
    const pending = profile.pendingNavigation;
    profile.unsavedChangesDialogOpen = false;
    profile.dirtyForms.clear();
    profile.pendingNavigation = null;
    await replayNavigation(pending);
  }

  function handleDirtyCancel() {
    profile.unsavedChangesDialogOpen = false;
    profile.switchTarget = null;
    profile.pendingNavigation = null;
  }
</script>

<SidebarProvider open={sidebarOpen} onOpenChange={handleOpenChange}>
  <AppSidebar
    setupMode={data.setup_mode}
    productionSetupComplete={data.production_setup_complete}
    shopName={data.shop_name ?? null}
  />
  <SidebarInset>
    <AppTopbar />
    <DisconnectBanner />
    <DemoBanner
      setupMode={data.setup_mode}
      hasProductionShop={data.production_setup_complete}
    />
    <RecoveryCodeWarning count={data.recovery_codes_remaining} />
    <main class="flex-1 p-4">
      {@render children()}
    </main>
  </SidebarInset>
  <UnsavedChangesDialog
    open={profile.unsavedChangesDialogOpen}
    description={dialogDescription}
    onconfirm={handleDirtyConfirm}
    oncancel={handleDirtyCancel}
  />
</SidebarProvider>
