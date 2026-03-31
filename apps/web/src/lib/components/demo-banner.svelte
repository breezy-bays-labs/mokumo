<script lang="ts">
  import { profile } from "$lib/stores/profile.svelte";

  interface Props {
    setupMode: "demo" | "production" | null;
    hasProductionShop: boolean;
  }

  let { setupMode, hasProductionShop }: Props = $props();

  let visible = $derived(setupMode === "demo");

  function openSwitcher() {
    profile.openProfileSwitcher = true;
  }
</script>

{#if visible}
  <div
    class="flex items-center justify-between border-b border-blue-200 bg-blue-50 px-4 py-2 text-sm text-blue-800 dark:border-blue-800 dark:bg-blue-950 dark:text-blue-200"
    role="status"
    data-testid="demo-banner"
  >
    <span>You're exploring demo data.</span>
    <button
      onclick={openSwitcher}
      class="ml-4 rounded-md bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-700 dark:bg-blue-700 dark:hover:bg-blue-600"
      data-testid="demo-banner-cta"
    >
      {hasProductionShop ? "Go to My Shop" : "Set Up My Shop"}
    </button>
  </div>
{/if}
