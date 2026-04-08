<script lang="ts">
  import { onMount } from "svelte";
  import { apiFetch } from "$lib/api";
  import type { BackupStatusResponse } from "$lib/types/BackupStatusResponse";

  let data = $state<BackupStatusResponse | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    const result = await apiFetch<BackupStatusResponse>("/api/backup-status");
    if (result.ok && "data" in result) {
      data = result.data;
    } else if (!result.ok) {
      error = result.error.message;
    }
    loading = false;
  });

  function dirname(path: string): string {
    const parts = path.replace(/\\/g, "/").split("/");
    parts.pop();
    return parts.join("/") || "/";
  }

  function basename(path: string): string {
    return path.replace(/\\/g, "/").split("/").pop() ?? path;
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-bold">Data Safety</h1>
    <p class="text-sm text-muted-foreground">
      Pre-migration backups are created automatically before each schema
      upgrade.
    </p>
  </div>

  {#if loading}
    <p class="text-sm text-muted-foreground">Loading backup status…</p>
  {:else if error}
    <p class="text-sm text-destructive">Failed to load backups: {error}</p>
  {:else if data}
    <!-- Production backups — high visual weight -->
    <div
      class="mx-auto max-w-md space-y-4 rounded-lg border-2 p-6"
      data-testid="production-backups-section"
    >
      <div>
        <h3 class="text-lg font-semibold">Production Backups</h3>
        <p class="text-sm text-muted-foreground">
          Your real shop data. These backups protect you during upgrades.
        </p>
      </div>

      {#if data.production.backups.length === 0}
        <p
          class="text-sm text-muted-foreground"
          data-testid="no-production-backups"
        >
          No backups yet. A backup will be taken automatically before your next
          upgrade.
        </p>
      {:else}
        <ul class="space-y-3" data-testid="production-backup-list">
          {#each data.production.backups as backup (backup.path)}
            <li
              class="rounded-md bg-muted p-3 text-sm"
              data-testid="production-backup-item"
            >
              <p class="font-mono text-xs break-all">{basename(backup.path)}</p>
              <p class="text-muted-foreground text-xs mt-1">
                {backup.backed_up_at}
              </p>
            </li>
          {/each}
        </ul>

        <div
          class="rounded-md border bg-muted/50 p-3 text-xs text-muted-foreground space-y-1"
        >
          <p class="font-medium">To restore from the most recent backup:</p>
          <ol class="list-decimal list-inside space-y-0.5">
            <li>Quit Mokumo.</li>
            <li>
              In your file manager, go to <code class="font-mono"
                >{dirname(data.production.backups[0].path)}</code
              >.
            </li>
            <li>
              Rename <code class="font-mono">mokumo.db</code> to
              <code class="font-mono">mokumo.db.broken</code>.
            </li>
            <li>
              Rename <code class="font-mono"
                >{basename(data.production.backups[0].path)}</code
              >
              to <code class="font-mono">mokumo.db</code>.
            </li>
            <li>Restart Mokumo.</li>
          </ol>
        </div>
      {/if}
    </div>

    <!-- Demo backups — de-emphasized -->
    <div
      class="mx-auto max-w-md space-y-3 rounded-lg border p-6 opacity-70"
      data-testid="demo-backups-section"
    >
      <div>
        <h3 class="text-base font-medium text-muted-foreground">
          Demo Backups
        </h3>
        <p class="text-sm text-muted-foreground">
          Demo data is ephemeral and low value. Set up your production profile
          to protect your real shop data.
        </p>
      </div>

      {#if data.demo.backups.length === 0}
        <p class="text-sm text-muted-foreground" data-testid="no-demo-backups">
          No backups yet.
        </p>
      {:else}
        <ul class="space-y-2" data-testid="demo-backup-list">
          {#each data.demo.backups as backup (backup.path)}
            <li
              class="rounded-md bg-muted/50 p-2 text-xs text-muted-foreground"
              data-testid="demo-backup-item"
            >
              <p class="font-mono break-all">{basename(backup.path)}</p>
              <p class="mt-0.5">{backup.backed_up_at}</p>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>
