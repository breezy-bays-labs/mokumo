<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { invalidateAll } from "$app/navigation";
  import { apiFetch } from "$lib/api";
  import type { ServerInfoResponse } from "$lib/types/ServerInfoResponse";
  import { profile } from "$lib/stores/profile.svelte";
  import * as Card from "$lib/components/ui/card";
  import { Badge, type BadgeVariant } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import CopyableUrl from "$lib/components/copyable-url.svelte";
  import { Switch } from "$lib/components/ui/switch";
  import { Label } from "$lib/components/ui/label";
  import Loader2 from "@lucide/svelte/icons/loader-2";
  import Wifi from "@lucide/svelte/icons/wifi";
  import WifiOff from "@lucide/svelte/icons/wifi-off";
  import type { LanAccessResponse } from "$lib/types/LanAccessResponse";

  let uploading = $state(false);
  let uploadError = $state<string | null>(null);

  function logoErrorMessage(code: string): string {
    switch (code) {
      case "logo_format_unsupported":
        return "Only PNG, JPEG, or WebP files are accepted.";
      case "logo_too_large":
        return "File is too large. Max 2 MB.";
      case "logo_dimensions_exceeded":
        return "Image is too large. Max 2048×2048 pixels.";
      case "logo_malformed":
        return "File unreadable. Try another image.";
      default:
        return "Upload failed. Please try again.";
    }
  }

  async function handleLogoUpload(files: FileList | null) {
    if (!files?.length) return;
    uploading = true;
    uploadError = null;
    try {
      const form = new FormData();
      form.append("logo", files[0]);
      const result = await apiFetch<never>("/api/shop/logo", {
        method: "POST",
        body: form,
      });
      if (!result.ok) {
        uploadError = logoErrorMessage(result.error.code);
      } else {
        await invalidateAll();
      }
    } finally {
      uploading = false;
    }
  }

  async function handleLogoRemove() {
    const result = await apiFetch<never>("/api/shop/logo", {
      method: "DELETE",
    });
    if (result.ok) {
      uploadError = null;
      await invalidateAll();
    } else {
      uploadError = "Could not remove logo. Please try again.";
    }
  }

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

  let lanEnabled = $state(false);
  let lanPrefSaving = $state(false);
  let lanPrefError = $state<string | null>(null);

  onMount(async () => {
    const [info, pref] = await Promise.all([
      apiFetch<ServerInfoResponse>("/api/server-info"),
      apiFetch<LanAccessResponse>("/api/settings/lan-access"),
    ]);
    if (!info.ok) {
      error = info.error.message;
    } else if ("data" in info) {
      serverInfo = info.data;
    } else {
      error = "Unexpected response from server. Please reload.";
    }
    if (pref.ok && "data" in pref) {
      lanEnabled = pref.data.enabled;
    } else if (!pref.ok) {
      lanPrefError = pref.error.message;
    } else {
      lanPrefError = "Unexpected LAN access response. Please reload.";
    }
    loading = false;
  });

  async function toggleLanAccess(next: boolean) {
    lanPrefError = null;
    lanPrefSaving = true;
    const prev = lanEnabled;
    lanEnabled = next;
    const result = await apiFetch<LanAccessResponse>(
      "/api/settings/lan-access",
      {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ enabled: next }),
      },
    );
    lanPrefSaving = false;
    if (!result.ok) {
      lanEnabled = prev;
      lanPrefError = result.error.message;
    }
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
      <Card.CardTitle>Shop Logo</Card.CardTitle>
      <Card.CardDescription>
        Square images 256×256 or larger work best.
      </Card.CardDescription>
    </Card.CardHeader>
    <Card.CardContent class="space-y-4">
      {#if uploadError}
        <p class="text-sm text-destructive">{uploadError}</p>
      {/if}
      <label for="shop-logo-upload" class="text-sm font-medium"
        >Upload logo</label
      >
      <input
        id="shop-logo-upload"
        type="file"
        accept="image/png,image/jpeg,image/webp"
        disabled={uploading}
        onchange={(e) => handleLogoUpload(e.currentTarget.files)}
      />
      {#if uploading}
        <Loader2 class="size-4 animate-spin" />
      {/if}
      {#if page.data.logo_url}
        <Button variant="outline" onclick={handleLogoRemove}>
          Remove logo
        </Button>
      {/if}
    </Card.CardContent>
  </Card.Card>

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
        <div
          class="flex items-center justify-between gap-4 rounded-md border p-3"
        >
          <div class="space-y-0.5">
            <Label for="lan-access-toggle" class="text-sm font-medium">
              Enable LAN Access
            </Label>
            <p class="text-xs text-muted-foreground">
              Advertise this server on your local network so shop devices can
              reach it by hostname.
            </p>
          </div>
          <Switch
            id="lan-access-toggle"
            checked={lanEnabled}
            disabled={lanPrefSaving}
            onCheckedChange={(v) => toggleLanAccess(v === true)}
            data-testid="lan-access-toggle"
          />
        </div>
        {#if lanPrefError}
          <p class="text-sm text-destructive">{lanPrefError}</p>
        {/if}

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
            <CopyableUrl
              url={serverInfo.lan_url}
              label="Copy LAN URL to clipboard"
              testId="copy-lan-url"
            />
          </div>
        {/if}

        {#if serverInfo.ip_url}
          <div class="space-y-1">
            <p class="text-sm font-medium">IP Address</p>
            <CopyableUrl
              url={serverInfo.ip_url}
              label="Copy IP address URL to clipboard"
              testId="copy-ip-url"
            />
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
