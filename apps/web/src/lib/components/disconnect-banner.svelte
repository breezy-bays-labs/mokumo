<script lang="ts">
  import { wsStatus } from "$lib/stores/ws-status.svelte";
  import Loader from "@lucide/svelte/icons/loader";
  import Check from "@lucide/svelte/icons/check";

  let visible = $derived(!wsStatus.connected || wsStatus.showReconnected);
</script>

{#if visible}
  <div
    data-testid="disconnect-banner"
    class="flex items-center justify-center gap-2 border-b bg-warning/10 px-4 py-2 text-sm"
    role="alert"
  >
    {#if wsStatus.showReconnected}
      <Check class="size-4 text-success" />
      <span data-testid="reconnected-text">Reconnected</span>
    {:else}
      {#if wsStatus.reconnecting}
        <Loader
          class="size-4 animate-spin"
          data-testid="reconnecting-indicator"
        />
      {/if}
      <span
        >Server disconnected — reconnecting automatically. If this persists,
        check with your shop admin.</span
      >
    {/if}
  </div>
{/if}
