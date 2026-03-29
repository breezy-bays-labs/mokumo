<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import HoverCardRoot from "./hover-card.svelte";
  import * as HoverCard from "./index.js";
  import * as Avatar from "$lib/components/ui/avatar/index.js";
  import Button from "$lib/components/ui/button/button.svelte";

  const { Story } = defineMeta({
    title: "UI/HoverCard",
    component: HoverCardRoot,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <HoverCard.Root>
    <HoverCard.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="link">@mokumo</Button>
      {/snippet}
    </HoverCard.Trigger>
    <HoverCard.Content class="w-80">
      <div class="flex justify-between space-x-4">
        <Avatar.Root>
          <Avatar.Fallback>MK</Avatar.Fallback>
        </Avatar.Root>
        <div class="space-y-1">
          <h4 class="text-sm font-semibold">Mokumo Print</h4>
          <p class="text-sm text-muted-foreground">
            Production management for decorated apparel shops.
          </p>
          <div class="flex items-center pt-2">
            <span class="text-xs text-muted-foreground">Self-hosted</span>
          </div>
        </div>
      </div>
    </HoverCard.Content>
  </HoverCard.Root>
</Story>

<Story name="With Avatar">
  <HoverCard.Root>
    <HoverCard.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="link">Hover for profile</Button>
      {/snippet}
    </HoverCard.Trigger>
    <HoverCard.Content class="w-80">
      <div class="flex justify-between space-x-4">
        <Avatar.Root class="h-12 w-12">
          <Avatar.Fallback>GB</Avatar.Fallback>
        </Avatar.Root>
        <div class="space-y-1">
          <h4 class="text-sm font-semibold">Gary's Shop</h4>
          <p class="text-sm text-muted-foreground">
            4Ink — Screen printing since 2015
          </p>
          <div class="flex items-center gap-4 pt-2">
            <div class="flex items-center text-xs text-muted-foreground">
              <span class="font-medium text-foreground">142</span>&nbsp;orders
            </div>
            <div class="flex items-center text-xs text-muted-foreground">
              <span class="font-medium text-foreground">23</span>&nbsp;customers
            </div>
          </div>
        </div>
      </div>
    </HoverCard.Content>
  </HoverCard.Root>
</Story>
