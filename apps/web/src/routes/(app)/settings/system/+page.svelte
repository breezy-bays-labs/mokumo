<script lang="ts">
  import { page } from "$app/state";
  import DemoResetDialog from "$lib/components/demo-reset-dialog.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import { Button } from "$lib/components/ui/button";
  import Badge from "$lib/components/ui/badge/badge.svelte";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Server from "@lucide/svelte/icons/server";

  let resetDialogOpen = $state(false);

  let isDemo = $derived(page.data.setup_mode === "demo");
</script>

<div class="space-y-6">
  <EmptyState
    icon={Server}
    title="System Settings"
    subtitle="Server configuration, backups, and system maintenance."
  />

  {#if isDemo}
    <div
      class="mx-auto max-w-md space-y-4 rounded-lg border p-6"
      data-testid="demo-mode-section"
    >
      <div class="flex items-center gap-2">
        <h3 class="text-lg font-semibold">Demo Mode</h3>
        <Badge variant="secondary">Active</Badge>
      </div>
      <p class="text-sm text-muted-foreground">
        You're running with demo data. Reset to restore the original sample
        data.
      </p>
      <Button variant="destructive" onclick={() => (resetDialogOpen = true)}>
        <RotateCcw class="mr-2 h-4 w-4" />
        Reset Demo Data
      </Button>
    </div>

    <DemoResetDialog
      bind:open={resetDialogOpen}
      onOpenChange={(open) => (resetDialogOpen = open)}
    />
  {/if}
</div>
