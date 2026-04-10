<script lang="ts">
  import { beforeNavigate, goto } from "$app/navigation";
  import UnsavedChangesDialog from "$lib/components/unsaved-changes-dialog.svelte";
  import { profile } from "$lib/stores/profile.svelte";

  let { children } = $props();

  beforeNavigate(({ cancel, to, willUnload }) => {
    if (willUnload) return;

    if (
      profile.pendingNavigation &&
      to?.url.href === profile.pendingNavigation
    ) {
      profile.pendingNavigation = null;
      return;
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

  async function handleConfirm() {
    const dest = profile.pendingNavigation;
    profile.unsavedChangesDialogOpen = false;
    profile.dirtyForms.clear();
    profile.pendingNavigation = null;
    if (dest) {
      try {
        await goto(dest);
      } catch {
        window.location.assign(dest);
      }
    }
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
