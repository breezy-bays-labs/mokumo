<script lang="ts">
  import { cn } from "$lib/utils.js";

  interface Props {
    title: string;
    description: string;
    class?: string;
  }

  let { title, description, class: className }: Props = $props();
</script>

<div class={cn("space-y-1", className)}>
  <h3 class="text-sm font-medium leading-none">{title}</h3>
  <p class="text-sm text-muted-foreground">{description}</p>
</div>
