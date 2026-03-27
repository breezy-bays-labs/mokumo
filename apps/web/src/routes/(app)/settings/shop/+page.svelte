<script lang="ts">
  import { onMount } from "svelte";
  import type { ServerInfoResponse } from "$lib/types/ServerInfoResponse";
  import * as Card from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { toast } from "$lib/components/toast";
  import Wifi from "@lucide/svelte/icons/wifi";
  import WifiOff from "@lucide/svelte/icons/wifi-off";
  import Copy from "@lucide/svelte/icons/copy";

  let serverInfo = $state<ServerInfoResponse | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      const res = await fetch("/api/server-info");
      if (!res.ok) throw new Error(`Server returned ${res.status}`);
      serverInfo = await res.json();
    } catch (e) {
      error = e instanceof Error ? e.message : "Failed to load";
    } finally {
      loading = false;
    }
  });

  async function copyUrl(url: string) {
    await navigator.clipboard.writeText(url);
    toast.success("URL copied to clipboard");
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-bold">Shop Settings</h1>
    <p class="text-sm text-muted-foreground">
      Configure your shop name, address, and branding.
    </p>
  </div>

  <Card.Card>
    <Card.CardHeader>
      <Card.CardTitle class="flex items-center gap-2">
        {#if serverInfo?.mdns_active}
          <Wifi class="size-5 text-status-success" />
        {:else}
          <WifiOff class="size-5 text-muted-foreground" />
        {/if}
        LAN Access
      </Card.CardTitle>
      <Card.CardDescription>
        Allow shop devices to access Mokumo on your local network.
      </Card.CardDescription>
    </Card.CardHeader>
    <Card.CardContent class="space-y-4">
      {#if loading}
        <div class="flex items-center gap-2">
          <div class="size-2.5 rounded-full bg-muted animate-pulse"></div>
          <span class="text-sm text-muted-foreground">Loading...</span>
        </div>
      {:else if error}
        <p class="text-sm text-destructive">{error}</p>
      {:else if serverInfo}
        <div class="flex items-center gap-2">
          {#if serverInfo.mdns_active}
            <Badge variant="default">Active</Badge>
          {:else}
            <Badge variant="secondary">Disabled</Badge>
          {/if}
        </div>

        {#if serverInfo.lan_url}
          <div class="space-y-1">
            <p class="text-sm font-medium">LAN URL</p>
            <div class="flex items-center gap-2">
              <code class="rounded bg-muted px-2 py-1 text-sm font-mono">
                {serverInfo.lan_url}
              </code>
              <Button
                variant="ghost"
                size="icon"
                onclick={() => copyUrl(serverInfo!.lan_url!)}
              >
                <Copy class="size-4" />
              </Button>
            </div>
          </div>
        {/if}

        <div class="space-y-1">
          <p class="text-sm font-medium">IP Address</p>
          <div class="flex items-center gap-2">
            <code class="rounded bg-muted px-2 py-1 text-sm font-mono">
              {serverInfo.ip_url}
            </code>
            <Button
              variant="ghost"
              size="icon"
              onclick={() => copyUrl(serverInfo!.ip_url)}
            >
              <Copy class="size-4" />
            </Button>
          </div>
        </div>

        <p class="text-sm text-muted-foreground">
          No authentication is configured. Anyone on your network can access
          this server.
        </p>
      {/if}
    </Card.CardContent>
  </Card.Card>
</div>
