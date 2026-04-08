<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { apiFetch } from "$lib/api";
  import type { ServerInfoResponse } from "$lib/types/ServerInfoResponse";
  import { profile } from "$lib/stores/profile.svelte";
  import * as Card from "$lib/components/ui/card";
  import { Badge, type BadgeVariant } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { toast } from "$lib/components/toast";
  import Wifi from "@lucide/svelte/icons/wifi";
  import WifiOff from "@lucide/svelte/icons/wifi-off";
  import Copy from "@lucide/svelte/icons/copy";

  let serverInfo = $state<ServerInfoResponse | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  type LanStatus = {
    label: "Active" | "Unavailable" | "Disabled";
    variant: BadgeVariant;
    badgeClass: string;
    iconClass: string;
    message: string;
  };

  const lanStatus = $derived.by<LanStatus | null>(() => {
    if (!serverInfo) return null;

    if (serverInfo.mdns_active) {
      return {
        label: "Active",
        variant: "outline",
        badgeClass: "border-success/40 bg-success/10 text-foreground",
        iconClass: "text-success",
        message:
          "Devices can discover this server by hostname on your local network.",
      };
    }

    if (serverInfo.ip_url) {
      return {
        label: "Unavailable",
        variant: "outline",
        badgeClass: "border-warning/40 bg-warning/10 text-foreground",
        iconClass: "text-warning",
        message:
          "mDNS discovery is unavailable. Use the IP address below to reach this server.",
      };
    }

    return {
      label: "Disabled",
      variant: "secondary",
      badgeClass: "",
      iconClass: "text-muted-foreground",
      message:
        "LAN discovery is disabled because this server is not available on the local network.",
    };
  });

  onMount(async () => {
    const result = await apiFetch<ServerInfoResponse>("/api/server-info");
    if (!result.ok) {
      error = result.error.message;
    } else if ("data" in result) {
      serverInfo = result.data;
    } else {
      error = "Unexpected response from server. Please reload.";
    }
    loading = false;
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
      Your shop details and network access.
    </p>
  </div>

  <Card.Card>
    <Card.CardHeader>
      <Card.CardTitle>Shop Name</Card.CardTitle>
    </Card.CardHeader>
    <Card.CardContent>
      {#if page.data.shop_name}
        {@const slug = page.data.shop_name
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, "-")
          .replace(/^-|-$/g, "")}
        <p class="text-sm font-medium">{page.data.shop_name}</p>
        <p class="text-sm text-muted-foreground font-mono">
          {slug || "shop"}.local
        </p>
      {:else}
        <p class="text-sm text-muted-foreground">No shop name set yet.</p>
        <Button
          variant="outline"
          class="mt-3"
          onclick={() => (profile.openProfileSwitcher = true)}
        >
          Switch to My Shop
        </Button>
      {/if}
    </Card.CardContent>
  </Card.Card>

  <Card.Card>
    <Card.CardHeader>
      <Card.CardTitle class="flex items-center gap-2">
        {#if lanStatus?.label === "Active"}
          <Wifi class={`size-5 ${lanStatus.iconClass}`} />
        {:else}
          <WifiOff
            class={`size-5 ${lanStatus?.iconClass ?? "text-muted-foreground"}`}
          />
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
      {:else if serverInfo && lanStatus}
        <div class="flex items-center gap-2">
          <Badge
            data-testid="lan-status-badge"
            variant={lanStatus.variant}
            class={lanStatus.badgeClass}
          >
            {lanStatus.label}
          </Badge>
        </div>

        <p class="text-sm text-muted-foreground">{lanStatus.message}</p>

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
                aria-label="Copy LAN URL to clipboard"
                data-testid="copy-lan-url"
                onclick={() => copyUrl(serverInfo!.lan_url!)}
              >
                <Copy class="size-4" />
              </Button>
            </div>
          </div>
        {/if}

        {#if serverInfo.ip_url}
          <div class="space-y-1">
            <p class="text-sm font-medium">IP Address</p>
            <div class="flex items-center gap-2">
              <code class="rounded bg-muted px-2 py-1 text-sm font-mono">
                {serverInfo.ip_url}
              </code>
              <Button
                variant="ghost"
                size="icon"
                aria-label="Copy IP address URL to clipboard"
                data-testid="copy-ip-url"
                onclick={() => copyUrl(serverInfo!.ip_url!)}
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
      {/if}
    </Card.CardContent>
  </Card.Card>
</div>
