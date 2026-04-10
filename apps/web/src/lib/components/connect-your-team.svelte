<script lang="ts">
  import type { ServerInfoResponse } from "$lib/types/ServerInfoResponse";
  import * as Card from "$lib/components/ui/card";
  import CopyableUrl from "$lib/components/copyable-url.svelte";
  import QrCode from "$lib/components/qr-code.svelte";
  import { Badge } from "$lib/components/ui/badge";

  let {
    serverInfo,
    isFirstLaunch = false,
  }: {
    serverInfo: ServerInfoResponse;
    isFirstLaunch?: boolean;
  } = $props();
</script>

<Card.Card
  data-testid="connect-your-team"
  class={isFirstLaunch ? "ring-2 ring-primary animate-pulse" : ""}
>
  <Card.CardHeader>
    <Card.CardTitle class="flex items-center gap-2">
      Connect Your Team
      {#if isFirstLaunch}
        <Badge variant="default">New</Badge>
      {/if}
    </Card.CardTitle>
    <Card.CardDescription>
      Share this with your team — they can open it in any browser on your shop
      WiFi.
    </Card.CardDescription>
  </Card.CardHeader>
  <Card.CardContent class="space-y-4">
    {#if serverInfo.ip_url}
      <div class="flex flex-col items-start gap-4 sm:flex-row sm:items-center">
        <QrCode value={serverInfo.ip_url} size={160} />
        <div class="space-y-3">
          <div class="space-y-1">
            <p class="text-sm font-medium">IP Address</p>
            <CopyableUrl
              url={serverInfo.ip_url}
              label="Copy connection link"
              testId="copy-team-url"
            />
          </div>
          {#if serverInfo.mdns_active && serverInfo.lan_url}
            <div class="space-y-1">
              <p class="text-sm font-medium">LAN URL</p>
              <CopyableUrl
                url={serverInfo.lan_url}
                label="Copy LAN URL to clipboard"
                testId="copy-lan-url"
              />
            </div>
          {/if}
        </div>
      </div>
    {/if}

    <div class="flex items-center gap-2">
      {#if serverInfo.mdns_active}
        <div
          class="size-2.5 rounded-full bg-status-success"
          data-testid="mdns-status-dot"
        ></div>
        <span class="text-sm" data-testid="mdns-status-text"
          >LAN discovery active</span
        >
      {:else}
        <div
          class="size-2.5 rounded-full bg-status-warning"
          data-testid="mdns-status-dot"
        ></div>
        <span
          class="text-sm text-muted-foreground"
          data-testid="mdns-status-text">Unavailable — use IP address</span
        >
      {/if}
    </div>

    {#if !serverInfo.mdns_active}
      <p
        class="text-sm text-muted-foreground"
        data-testid="troubleshooting-text"
      >
        LAN discovery may be blocked by AP isolation or multicast filtering on
        your network.{#if serverInfo.ip_url}
          Use the IP address above instead.{/if}
      </p>
    {/if}
  </Card.CardContent>
</Card.Card>
