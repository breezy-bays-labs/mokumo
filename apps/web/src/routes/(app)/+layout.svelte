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

  // Cancel any navigation that fires while the unsaved-changes dialog is open.
  // This prevents the dialog state from being torn down mid-confirmation.
  beforeNavigate(({ cancel }) => {
    if (profile.unsavedChangesDialogOpen) {
      cancel();
    }
  });

  let confirmSwitching = $state(false);

  async function handleDirtyConfirm() {
    if (confirmSwitching) return;
    const target = profile.switchTarget;
    if (!target) return;
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
  }

  function handleDirtyCancel() {
    profile.unsavedChangesDialogOpen = false;
    profile.switchTarget = null;
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
    onconfirm={handleDirtyConfirm}
    oncancel={handleDirtyCancel}
  />
</SidebarProvider>
