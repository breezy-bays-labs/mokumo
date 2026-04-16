<script lang="ts">
  import type { DiagnosticsResponse } from "$lib/types/kikan/DiagnosticsResponse";
  import * as Card from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import Copy from "@lucide/svelte/icons/copy";
  import Check from "@lucide/svelte/icons/check";
  import Download from "@lucide/svelte/icons/download";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";

  let diagnostics = $state<DiagnosticsResponse | null>(null);
  let loadError = $state<string | null>(null);
  let loading = $state(true);
  let copied = $state(false);
  let downloading = $state(false);
  let downloadError = $state<string | null>(null);

  let requestId = 0;
  let copyResetTimeout: ReturnType<typeof setTimeout> | null = null;

  async function load() {
    const currentRequest = ++requestId;
    loading = true;
    loadError = null;
    try {
      const res = await fetch("/api/diagnostics", {
        headers: { Accept: "application/json" },
      });
      if (currentRequest !== requestId) return;
      if (!res.ok) {
        loadError = `HTTP ${res.status}`;
        diagnostics = null;
        return;
      }
      const data = (await res.json()) as DiagnosticsResponse;
      if (currentRequest !== requestId) return;
      diagnostics = data;
    } catch (e) {
      if (currentRequest !== requestId) return;
      loadError = e instanceof Error ? e.message : "Network error";
      diagnostics = null;
    } finally {
      if (currentRequest === requestId) loading = false;
    }
  }

  $effect(() => {
    void load();
  });

  function formatBytes(bytes: number | null): string {
    if (bytes === null || bytes === undefined) return "unknown";
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024)
      return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function formatUptime(seconds: number): string {
    const d = Math.floor(seconds / 86400);
    const h = Math.floor((seconds % 86400) / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    if (d > 0) return `${d}d ${h}h ${m}m`;
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  }

  function toMarkdown(d: DiagnosticsResponse): string {
    const lines = [
      `# Mokumo Diagnostics`,
      ``,
      `## App`,
      `- Name: ${d.app.name}`,
      `- Version: ${d.app.version}`,
      `- Build commit: ${d.app.build_commit ?? "unknown"}`,
      ``,
      `## Runtime`,
      `- Active profile: ${d.runtime.active_profile}`,
      `- Setup complete: ${d.runtime.setup_complete}`,
      `- First launch: ${d.runtime.is_first_launch}`,
      `- Uptime: ${formatUptime(d.runtime.uptime_seconds)}`,
      `- Host: ${d.runtime.host}:${d.runtime.port}`,
      `- mDNS active: ${d.runtime.mdns_active}`,
      `- LAN URL: ${d.runtime.lan_url ?? "none"}`,
      ``,
      `## Database`,
      `### Production`,
      `- Schema version: ${d.database.production.schema_version}`,
      `- File size: ${formatBytes(d.database.production.file_size_bytes)}`,
      `- WAL mode: ${d.database.production.wal_mode}`,
      `### Demo`,
      `- Schema version: ${d.database.demo.schema_version}`,
      `- File size: ${formatBytes(d.database.demo.file_size_bytes)}`,
      `- WAL mode: ${d.database.demo.wal_mode}`,
      ``,
      `## OS`,
      `- Family: ${d.os.family}`,
      `- Arch: ${d.os.arch}`,
      ``,
      `## System`,
      `- Hostname: ${d.system.hostname ?? "unknown"}`,
      `- Memory: ${formatBytes(d.system.used_memory_bytes)} / ${formatBytes(d.system.total_memory_bytes)}`,
      `- Disk free: ${formatBytes(d.system.disk_free_bytes)} free of ${formatBytes(d.system.disk_total_bytes)}`,
    ];
    return lines.join("\n");
  }

  async function copyMarkdown() {
    if (!diagnostics) return;
    try {
      await navigator.clipboard.writeText(toMarkdown(diagnostics));
      copied = true;
      if (copyResetTimeout !== null) clearTimeout(copyResetTimeout);
      copyResetTimeout = setTimeout(() => {
        copied = false;
        copyResetTimeout = null;
      }, 2000);
    } catch (e) {
      // Clipboard access may be denied — that's acceptable. Log unexpected errors
      // so bugs in toMarkdown() are not swallowed.
      console.error("Copy to clipboard failed:", e);
    }
  }

  async function exportBundle() {
    downloading = true;
    downloadError = null;
    try {
      const res = await fetch("/api/diagnostics/bundle", {
        headers: { Accept: "application/zip" },
      });
      if (!res.ok) {
        let message = `Export failed (HTTP ${res.status})`;
        try {
          const err = (await res.json()) as { message?: string };
          if (err.message) message = err.message;
        } catch (parseErr) {
          console.error("Failed to parse error response body:", parseErr);
        }
        downloadError = message;
        return;
      }
      const blob = await res.blob();
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `mokumo-diagnostics-${new Date().toISOString().slice(0, 10)}.zip`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      window.URL.revokeObjectURL(url);
    } catch (e) {
      console.error("Diagnostics bundle export failed:", e);
      downloadError = e instanceof Error ? e.message : "Network error";
    } finally {
      downloading = false;
    }
  }
</script>

<Card.Card data-testid="diagnostics-card" class="mx-auto max-w-md">
  <Card.CardHeader>
    <Card.CardTitle>Diagnostics</Card.CardTitle>
    <Card.CardDescription>
      Version and runtime state. Share this with support if something goes
      wrong.
    </Card.CardDescription>
  </Card.CardHeader>
  <Card.CardContent class="space-y-4">
    {#if loading}
      <p class="text-sm text-muted-foreground">Loading…</p>
    {:else if loadError}
      <div class="space-y-2">
        <p class="text-sm text-destructive" data-testid="diagnostics-error">
          Could not load diagnostics: {loadError}
        </p>
        <Button variant="outline" size="sm" onclick={() => load()}>
          <RotateCw class="mr-2 h-4 w-4" />
          Retry
        </Button>
      </div>
    {:else if diagnostics}
      <dl
        class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm"
        data-testid="diagnostics-fields"
      >
        <dt class="text-muted-foreground">App</dt>
        <dd data-testid="diag-app">
          {diagnostics.app.name}
          {diagnostics.app.version}
        </dd>

        <dt class="text-muted-foreground">Profile</dt>
        <dd data-testid="diag-profile">{diagnostics.runtime.active_profile}</dd>

        <dt class="text-muted-foreground">Uptime</dt>
        <dd data-testid="diag-uptime">
          {formatUptime(diagnostics.runtime.uptime_seconds)}
        </dd>

        <dt class="text-muted-foreground">Host</dt>
        <dd>{diagnostics.runtime.host}:{diagnostics.runtime.port}</dd>

        <dt class="text-muted-foreground">Production DB</dt>
        <dd data-testid="diag-prod-db">
          schema v{diagnostics.database.production.schema_version} ·
          {formatBytes(diagnostics.database.production.file_size_bytes)}
          {#if diagnostics.database.production.wal_mode}
            <span> · WAL on</span>
          {:else}
            <span class="text-destructive"> · WAL off</span>
          {/if}
        </dd>

        <dt class="text-muted-foreground">Demo DB</dt>
        <dd data-testid="diag-demo-db">
          schema v{diagnostics.database.demo.schema_version} ·
          {formatBytes(diagnostics.database.demo.file_size_bytes)}
          {#if diagnostics.database.demo.wal_mode}
            <span> · WAL on</span>
          {:else}
            <span class="text-destructive"> · WAL off</span>
          {/if}
        </dd>

        <dt class="text-muted-foreground">OS</dt>
        <dd>{diagnostics.os.family} ({diagnostics.os.arch})</dd>

        <dt class="text-muted-foreground">Memory</dt>
        <dd data-testid="diag-memory">
          {formatBytes(diagnostics.system.used_memory_bytes)} / {formatBytes(
            diagnostics.system.total_memory_bytes,
          )}
        </dd>

        <dt class="text-muted-foreground">Disk free</dt>
        <dd data-testid="diag-disk">
          {formatBytes(diagnostics.system.disk_free_bytes)} free of {formatBytes(
            diagnostics.system.disk_total_bytes,
          )}
        </dd>
      </dl>

      <div class="flex flex-wrap gap-2">
        <Button
          variant="outline"
          size="sm"
          onclick={copyMarkdown}
          data-testid="diagnostics-copy"
        >
          {#if copied}
            <Check class="mr-2 h-4 w-4" />
            Copied
          {:else}
            <Copy class="mr-2 h-4 w-4" />
            Copy as Markdown
          {/if}
        </Button>

        <Button
          variant="outline"
          size="sm"
          onclick={exportBundle}
          disabled={downloading}
          data-testid="diagnostics-export"
        >
          {#if downloading}
            <RotateCw class="mr-2 h-4 w-4 animate-spin" />
            Preparing…
          {:else}
            <Download class="mr-2 h-4 w-4" />
            Export Bundle
          {/if}
        </Button>
      </div>

      {#if downloadError}
        <p
          class="text-sm text-destructive"
          data-testid="diagnostics-export-error"
        >
          {downloadError}
        </p>
      {/if}

      <p class="text-xs text-muted-foreground">
        Bundle contains app logs and runtime metadata. No customer data is
        included.
      </p>
    {/if}
  </Card.CardContent>
</Card.Card>
