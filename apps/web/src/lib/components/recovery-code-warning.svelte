<script lang="ts">
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import X from "@lucide/svelte/icons/x";

  interface Props {
    count: number;
  }

  let { count }: Props = $props();

  let dismissed = $state(false);

  $effect(() => {
    if (typeof window !== "undefined") {
      dismissed =
        sessionStorage.getItem("recovery-warning-dismissed") === "true";
    }
  });

  function dismiss() {
    sessionStorage.setItem("recovery-warning-dismissed", "true");
    dismissed = true;
  }

  let visible = $derived(count < 3 && !dismissed);
  let message = $derived(
    count === 0
      ? "You have no recovery codes remaining."
      : `You have ${count} recovery code${count === 1 ? "" : "s"} remaining.`,
  );
</script>

{#if visible}
  <div
    class="flex items-center justify-between border-b border-amber-200 bg-amber-50 px-4 py-2 text-sm text-amber-800 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-200"
    role="alert"
  >
    <div class="flex items-center gap-2">
      <TriangleAlert class="h-4 w-4 shrink-0" />
      <span>
        {message}
        <a
          href="/settings/account"
          class="font-medium underline underline-offset-4 hover:no-underline"
        >
          Regenerate in Settings
        </a>
      </span>
    </div>
    <button
      onclick={dismiss}
      class="ml-2 rounded-sm p-0.5 hover:bg-amber-200/50 dark:hover:bg-amber-800/50"
      aria-label="Dismiss warning"
    >
      <X class="h-4 w-4" />
    </button>
  </div>
{/if}
