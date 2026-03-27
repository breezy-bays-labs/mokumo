<script lang="ts">
  import { page } from "$app/state";
  import { cn } from "$lib/utils";

  interface Tab {
    label: string;
    href: string;
    count?: number;
  }

  interface Props {
    tabs: Tab[];
    class?: string;
  }

  let { tabs, class: className }: Props = $props();

  function isActive(href: string): boolean {
    const path = page.url.pathname;
    return path === href || path === href + "/";
  }
</script>

<nav
  class={cn("flex overflow-x-auto border-b", className)}
  aria-label="Tab navigation"
>
  {#each tabs as tab (tab.href)}
    <a
      href={tab.href}
      class={cn(
        "whitespace-nowrap border-b-2 px-4 py-2.5 text-sm font-medium transition-colors",
        isActive(tab.href)
          ? "border-primary text-primary"
          : "border-transparent text-muted-foreground hover:border-muted-foreground/30 hover:text-foreground",
      )}
      aria-current={isActive(tab.href) ? "page" : undefined}
    >
      {tab.label}
      {#if tab.count != null}
        <span class="ml-1.5 rounded-full bg-muted px-2 py-0.5 text-xs">
          {tab.count}
        </span>
      {/if}
    </a>
  {/each}
</nav>
