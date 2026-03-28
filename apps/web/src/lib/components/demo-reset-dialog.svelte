<script lang="ts">
  import * as AlertDialog from "$lib/components/ui/alert-dialog";
  import Loader from "@lucide/svelte/icons/loader";

  interface Props {
    open: boolean;
    onOpenChange: (open: boolean) => void;
  }

  let { open = $bindable(), onOpenChange }: Props = $props();

  let resetting = $state(false);
  let error = $state<string | null>(null);

  async function handleReset() {
    resetting = true;
    error = null;

    try {
      const res = await fetch("/api/demo/reset", { method: "POST" });
      if (!res.ok) {
        const body = await res.json().catch(() => null);
        error = body?.message ?? "Failed to reset demo data";
        resetting = false;
        return;
      }

      // Clear banner dismissal so it shows again after reload
      localStorage.removeItem("demo_banner_dismissed");

      // Server will restart — wait briefly then reload
      await new Promise((resolve) => setTimeout(resolve, 1500));
      window.location.reload();
    } catch {
      error = "Connection lost. The server may be restarting.";
      // Try reloading after a delay — server should be back
      setTimeout(() => window.location.reload(), 3000);
    }
  }
</script>

<AlertDialog.Root bind:open {onOpenChange}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>Reset Demo Data</AlertDialog.Title>
      <AlertDialog.Description>
        This will erase all changes to demo data and restore the original sample
        data. The application will restart.
      </AlertDialog.Description>
    </AlertDialog.Header>

    {#if error}
      <p class="text-sm text-destructive">{error}</p>
    {/if}

    <AlertDialog.Footer>
      <AlertDialog.Cancel disabled={resetting}>Cancel</AlertDialog.Cancel>
      <AlertDialog.Action onclick={handleReset} disabled={resetting}>
        {#if resetting}
          <Loader class="mr-2 h-4 w-4 animate-spin" />
          Resetting...
        {:else}
          Reset
        {/if}
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
