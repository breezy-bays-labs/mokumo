<script lang="ts">
  import type { Snippet } from "svelte";
  import { beforeNavigate, goto } from "$app/navigation";
  import AppSidebar from "$lib/components/app-sidebar.svelte";
  import AppTopbar from "$lib/components/app-topbar.svelte";
  import DemoBanner from "$lib/components/demo-banner.svelte";
  import RecoveryCodeWarning from "$lib/components/recovery-code-warning.svelte";
  import UnsavedChangesDialog from "$lib/components/unsaved-changes-dialog.svelte";
  import { SidebarInset, SidebarProvider } from "$lib/components/ui/sidebar";
  import { apiFetch } from "$lib/api";
  import { profile } from "$lib/stores/profile.svelte";
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

  // Guard against navigating away with unsaved form changes.
  // 1. If confirmed replay (user clicked "Leave anyway") → allow through.
  // 2. If dirty forms and no dialog open → cancel, store destination, show dialog.
  // 3. If dialog already open → cancel (race guard).
  beforeNavigate(({ cancel, to, willUnload }) => {
    if (willUnload) return; // beforeunload handles tab close / external nav

    if (
      profile.pendingNavigation &&
      to?.url.href === profile.pendingNavigation
    ) {
      profile.pendingNavigation = null;
      return; // confirmed replay — allow
    }

    if (profile.dirtyForms.size > 0 && !profile.unsavedChangesDialogOpen) {
      cancel();
      profile.pendingNavigation = to?.url.href ?? null;
      profile.unsavedChangesDialogOpen = true;
      return;
    }

    if (profile.unsavedChangesDialogOpen) {
      cancel();
    }
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

    // Navigation context
    const dest = profile.pendingNavigation;
    profile.unsavedChangesDialogOpen = false;
    profile.dirtyForms.clear();
    profile.pendingNavigation = null;
    if (dest) {
      try {
        await goto(dest);
      } catch (error) {
        console.error("Navigation replay failed:", error);
        window.location.assign(dest);
      }
    }
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
