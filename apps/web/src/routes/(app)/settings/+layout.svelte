<script lang="ts">
  import { page } from "$app/state";

  let { children } = $props();

  const tabs = [
    { label: "Shop", href: "/settings/shop" },
    { label: "Account", href: "/settings/account" },
    { label: "System", href: "/settings/system" },
    { label: "Data Safety", href: "/settings/data-safety" },
  ];

  const activeTab = $derived(page.url.pathname);
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-2xl font-bold">Settings</h1>
  </div>

  <nav class="flex gap-1 border-b">
    {#each tabs as tab (tab.href)}
      <a
        href={tab.href}
        class="px-4 py-2 text-sm font-medium transition-colors border-b-2 -mb-px {activeTab.startsWith(
          tab.href,
        )
          ? 'border-primary text-foreground'
          : 'border-transparent text-muted-foreground hover:text-foreground'}"
      >
        {tab.label}
      </a>
    {/each}
  </nav>

  {@render children()}
</div>
