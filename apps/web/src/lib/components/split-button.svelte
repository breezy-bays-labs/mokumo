<script lang="ts">
  import { cn } from "$lib/utils.js";
  import Button from "$lib/components/ui/button/button.svelte";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import type { Snippet } from "svelte";

  type Variant = "default" | "secondary" | "outline" | "destructive";

  interface Props {
    label: string;
    variant?: Variant;
    disabled?: boolean;
    onclick?: () => void;
    items: Snippet;
    class?: string;
  }

  let {
    label,
    variant = "default",
    disabled = false,
    onclick,
    items,
    class: className,
  }: Props = $props();
</script>

<div class={cn("inline-flex items-center rounded-md shadow-xs", className)}>
  <Button {variant} {disabled} {onclick} class="rounded-r-none">
    {label}
  </Button>
  <DropdownMenu.Root>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <Button
          {...props}
          {variant}
          {disabled}
          size="icon"
          class="h-8 w-8 rounded-l-none border-l border-background/20"
          aria-label="More options"
        >
          <ChevronDown class="size-4" />
        </Button>
      {/snippet}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content align="end">
      {@render items()}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
</div>
