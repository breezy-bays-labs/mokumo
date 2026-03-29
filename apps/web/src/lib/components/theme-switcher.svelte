<script lang="ts">
  import { cn } from "$lib/utils.js";
  import { mode, toggleMode } from "mode-watcher";
  import Sun from "@lucide/svelte/icons/sun";
  import Moon from "@lucide/svelte/icons/moon";

  interface Props {
    class?: string;
  }

  let { class: className }: Props = $props();
</script>

<button
  type="button"
  onclick={toggleMode}
  class={cn(
    "inline-flex h-9 w-9 items-center justify-center rounded-md border border-input bg-background text-muted-foreground transition-colors",
    "hover:bg-accent hover:text-accent-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
    className,
  )}
  aria-label="Toggle theme"
>
  {#if mode.current === "dark"}
    <Moon class="size-4" />
  {:else}
    <Sun class="size-4" />
  {/if}
</button>
