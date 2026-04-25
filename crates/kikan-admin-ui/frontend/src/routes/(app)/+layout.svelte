<script lang="ts">
  import { page } from "$app/state";
  import { base } from "$app/paths";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Topbar from "$lib/components/Topbar.svelte";
  import { applyTokensToRoot } from "$lib/branding";
  import { fetchPlatform } from "$lib/platform";
  import { navEntries, isActive } from "$lib/nav";

  let { data, children } = $props();

  let branding = $derived(data.branding);
  let runningShops = $state(1);
  let activeEntry = $derived(navEntries.find((e) => isActive(page.url.pathname, e, base)));
  let pageTitle = $derived(
    activeEntry ? `${activeEntry.label} · ${branding.appName} Admin` : `${branding.appName} Admin`,
  );

  async function refreshAppMeta(signal: AbortSignal): Promise<void> {
    try {
      const meta = await fetchPlatform<{ running_shops: number }>("/app-meta", { signal });
      if (!signal.aborted) runningShops = meta.running_shops;
    } catch {
      // Leave the previous value; the chrome still renders.
    }
  }

  $effect(() => {
    applyTokensToRoot(branding.tokens);
  });

  $effect(() => {
    const controller = new AbortController();
    void refreshAppMeta(controller.signal);
    const timer = setInterval(() => refreshAppMeta(controller.signal), 1500);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  });
</script>

<svelte:head>
  <title>{pageTitle}</title>
</svelte:head>

<div class="flex h-full min-h-screen w-full">
  <Sidebar currentPath={page.url.pathname} {branding} />

  <div class="flex flex-1 flex-col">
    <Topbar {branding} {runningShops} />
    <main class="flex-1 overflow-auto bg-background">
      {@render children()}
    </main>
  </div>
</div>
