<script lang="ts">
  import type { Component } from "svelte";

  interface Props {
    icon: Component<Record<string, unknown>>;
    title: string;
    subtitle: string;
  }

  let { icon: Icon, title, subtitle }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center py-24 text-center">
  <Icon class="size-12 text-muted-foreground mb-4" />
  <h2 class="text-xl font-semibold">{title}</h2>
  <p class="mt-2 text-sm text-muted-foreground max-w-md">{subtitle}</p>
</div>
