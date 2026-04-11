<script lang="ts">
  import Store from "@lucide/svelte/icons/store";

  interface Props {
    mode: "demo" | "production" | null;
    logo_url: string | null;
    size?: "sm" | "md";
    class?: string;
  }

  let { mode, logo_url, size = "md", class: className = "" }: Props = $props();

  let showGlyph = $state(false);

  // Reset stale onerror state when logo_url changes (e.g. after re-upload).
  $effect(() => {
    void logo_url;
    showGlyph = false;
  });

  const sizeClass = $derived(size === "sm" ? "h-5 w-auto" : "h-8 w-auto");
</script>

{#if mode === "demo"}
  <img
    src="/mokumo-cloud.png"
    alt=""
    class="{sizeClass} shrink-0 dark:invert {className}"
    draggable="false"
  />
{:else if logo_url && !showGlyph}
  <img
    src={logo_url}
    alt=""
    class="{sizeClass} shrink-0 {className}"
    draggable="false"
    onerror={() => (showGlyph = true)}
  />
{:else}
  <Store class="{size === 'sm' ? 'size-5' : 'size-8'} {className}" />
{/if}
