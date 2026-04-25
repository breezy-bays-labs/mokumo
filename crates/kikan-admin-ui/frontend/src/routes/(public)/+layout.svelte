<script lang="ts">
  import { applyTokensToRoot } from "$lib/branding";
  import ConnectionMonitor from "$lib/components/ConnectionMonitor.svelte";

  let { data, children } = $props();
  let branding = $derived(data.branding);

  $effect(() => {
    applyTokensToRoot(branding.tokens);
  });
</script>

<svelte:head>
  <title>{branding.appName} Admin</title>
</svelte:head>

<div
  data-testid="public-shell"
  class="flex min-h-screen flex-col bg-background text-foreground"
  style:--brand-bg={branding.tokens.bg}
  style:--brand-fg={branding.tokens.fg}
  style:--brand-primary={branding.tokens.primary}
  style:--brand-accent={branding.tokens.accent}
>
  <header
    data-testid="public-header"
    class="flex h-14 items-center border-b border-border bg-background px-6"
  >
    <span class="text-base font-semibold text-foreground">{branding.appName}</span>
    <span class="ml-2 text-xs text-muted-foreground">Admin</span>
  </header>

  <div class="px-6 pt-3">
    <ConnectionMonitor />
  </div>

  <main class="flex flex-1 items-start justify-center p-8">
    {@render children()}
  </main>
</div>
