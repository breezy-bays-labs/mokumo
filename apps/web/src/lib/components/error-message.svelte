<script lang="ts">
  import { cn } from "$lib/utils.js";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";

  type Size = "sm" | "md" | "lg";

  interface Props {
    message: string;
    label?: string;
    size?: Size;
    class?: string;
  }

  let { message, label, size = "md", class: className }: Props = $props();

  const sizeMap: Record<Size, { icon: string; text: string }> = {
    sm: { icon: "size-3.5", text: "text-xs" },
    md: { icon: "size-4", text: "text-sm" },
    lg: { icon: "size-5", text: "text-base" },
  };
</script>

<div
  class={cn("flex items-start gap-2 text-error", sizeMap[size].text, className)}
  role="alert"
>
  <CircleAlert class={cn("shrink-0 mt-0.5", sizeMap[size].icon)} />
  <span>
    {#if label}
      <span class="mr-1 font-medium">{label}</span>
    {/if}
    {message}
  </span>
</div>
