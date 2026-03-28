<script lang="ts">
  import { browser } from "$app/environment";
  import X from "@lucide/svelte/icons/x";

  interface Props {
    setupMode: string | null;
  }

  let { setupMode }: Props = $props();

  const STORAGE_KEY = "demo_banner_dismissed";

  let dismissed = $state(
    browser && localStorage.getItem(STORAGE_KEY) === "true",
  );

  let visible = $derived(setupMode === "demo" && !dismissed);

  function dismiss() {
    localStorage.setItem(STORAGE_KEY, "true");
    dismissed = true;
  }
</script>

{#if visible}
  <div
    class="flex items-center justify-between border-b border-blue-200 bg-blue-50 px-4 py-2 text-sm text-blue-800 dark:border-blue-800 dark:bg-blue-950 dark:text-blue-200"
    role="status"
    data-testid="demo-banner"
  >
    <div class="flex items-center gap-2">
      <span>
        You're exploring demo data.
        <a
          href="/settings/system"
          class="font-medium underline underline-offset-4 hover:no-underline"
        >
          Go to Settings
        </a>
      </span>
    </div>
    <button
      onclick={dismiss}
      class="ml-2 rounded-sm p-0.5 hover:bg-blue-200/50 dark:hover:bg-blue-800/50"
      aria-label="Dismiss demo banner"
    >
      <X class="h-4 w-4" />
    </button>
  </div>
{/if}
