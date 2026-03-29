<script lang="ts">
  import { cn } from "$lib/utils.js";
  import * as Tooltip from "$lib/components/ui/tooltip/index.js";
  import type { Snippet } from "svelte";

  interface NavItem {
    label: string;
    href: string;
    icon: Snippet;
    active?: boolean;
  }

  interface Props {
    items: NavItem[];
    class?: string;
  }

  let { items, class: className }: Props = $props();
</script>

<aside
  class={cn(
    "flex h-full w-14 flex-col items-center gap-1 border-r bg-sidebar py-4",
    className,
  )}
>
  <nav aria-label="Primary">
    {#each items as item (item.href)}
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <a
              {...props}
              href={item.href}
              class={cn(
                "flex size-10 items-center justify-center rounded-lg text-sidebar-foreground transition-colors",
                "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
                item.active &&
                  "bg-sidebar-accent text-sidebar-accent-foreground",
              )}
              aria-label={item.label}
              aria-current={item.active ? "page" : undefined}
            >
              {@render item.icon()}
            </a>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content side="right">
          {item.label}
        </Tooltip.Content>
      </Tooltip.Root>
    {/each}
  </nav>
</aside>
