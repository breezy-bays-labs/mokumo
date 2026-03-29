<script lang="ts">
  import { cn } from "$lib/utils.js";

  type Size = "sm" | "md" | "lg";

  interface Props {
    size?: Size;
    class?: string;
  }

  let { size = "md", class: className }: Props = $props();

  const sizeMap: Record<Size, string> = {
    sm: "size-4 border-[1.5px]",
    md: "size-5 border-2",
    lg: "size-8 border-[3px]",
  };
</script>

<span
  class={cn(
    "inline-block animate-spin rounded-full border-current border-r-transparent",
    sizeMap[size],
    className,
  )}
  role="status"
  aria-label="Loading"
>
  <span class="sr-only">Loading...</span>
</span>
