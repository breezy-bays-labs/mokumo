<script lang="ts">
  import UnsavedChangesDialog from "$lib/components/unsaved-changes-dialog.svelte";
  import {
    installNavigationGuard,
    replayNavigation,
  } from "$lib/navigation-guard";
  import { profile } from "$lib/stores/profile.svelte";

  let { children } = $props();

  installNavigationGuard();

  async function handleConfirm() {
    const pending = profile.pendingNavigation;
    profile.unsavedChangesDialogOpen = false;
    profile.dirtyForms.clear();
    profile.pendingNavigation = null;
    await replayNavigation(pending);
  }

  function handleCancel() {
    profile.unsavedChangesDialogOpen = false;
    profile.pendingNavigation = null;
  }
</script>

<div class="flex min-h-screen items-center justify-center bg-background p-4">
  <div class="w-full max-w-md space-y-6">
    <div class="flex flex-col items-center gap-2 text-center">
      <img
        src="/mokumo-cloud.png"
        alt="Mokumo"
        class="h-16 dark:invert select-none"
        draggable="false"
        oncontextmenu={(e) => e.preventDefault()}
      />
      <span class="text-lg font-semibold tracking-tight">Mokumo Print</span>
    </div>
    {@render children()}
  </div>
</div>
<UnsavedChangesDialog
  open={profile.unsavedChangesDialogOpen}
  onconfirm={handleConfirm}
  oncancel={handleCancel}
/>
