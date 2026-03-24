<script lang="ts">
  import { onMount } from "svelte";
  import type { HealthResponse } from "$lib/types/HealthResponse";
  import * as Card from "$lib/components/ui/card";

  let healthy = $state<boolean | null>(null);
  let version = $state("");
  let lanUrl = $state("");

  onMount(async () => {
    lanUrl = window.location.origin;
    try {
      const res = await fetch("/api/health");
      if (!res.ok) {
        healthy = false;
        return;
      }
      const data: HealthResponse = await res.json();
      healthy = true;
      version = data.version;
    } catch {
      healthy = false;
    }
  });
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-bold">Your Shop</h1>
    <p class="text-sm text-muted-foreground">Powered by Mokumo</p>
  </div>

  <div class="grid gap-4 md:grid-cols-3">
    <Card.Card>
      <Card.CardHeader class="pb-2">
        <Card.CardTitle class="text-sm font-medium"
          >Server Status</Card.CardTitle
        >
      </Card.CardHeader>
      <Card.CardContent>
        <div class="flex items-center gap-2">
          {#if healthy === null}
            <div class="size-2.5 rounded-full bg-muted animate-pulse"></div>
            <span class="text-sm text-muted-foreground">Checking...</span>
          {:else if healthy}
            <div class="size-2.5 rounded-full bg-status-success"></div>
            <span class="text-sm">Online</span>
          {:else}
            <div class="size-2.5 rounded-full bg-status-error"></div>
            <span class="text-sm">Offline</span>
          {/if}
        </div>
      </Card.CardContent>
    </Card.Card>

    <Card.Card>
      <Card.CardHeader class="pb-2">
        <Card.CardTitle class="text-sm font-medium">Version</Card.CardTitle>
      </Card.CardHeader>
      <Card.CardContent>
        <p class="text-sm">{version || "—"}</p>
      </Card.CardContent>
    </Card.Card>

    <Card.Card>
      <Card.CardHeader class="pb-2">
        <Card.CardTitle class="text-sm font-medium">LAN URL</Card.CardTitle>
      </Card.CardHeader>
      <Card.CardContent>
        <p class="text-sm font-mono">{lanUrl || "—"}</p>
      </Card.CardContent>
    </Card.Card>
  </div>

  <Card.Card>
    <Card.CardHeader>
      <Card.CardTitle>Getting Started</Card.CardTitle>
      <Card.CardDescription>Start building your shop</Card.CardDescription>
    </Card.CardHeader>
    <Card.CardContent>
      <a href="/customers" class="text-sm text-primary hover:underline">
        Create your first customer &rarr;
      </a>
    </Card.CardContent>
  </Card.Card>
</div>
