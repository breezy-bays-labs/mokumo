<script lang="ts">
  import { cn } from "$lib/utils.js";
  import X from "@lucide/svelte/icons/x";
  import type { Snippet } from "svelte";

  type Variant = "success" | "warning" | "error" | "info";

  interface Props {
    variant?: Variant;
    message: string;
    action?: Snippet;
    dismissible?: boolean;
    ondismiss?: () => void;
    class?: string;
  }

  let {
    variant = "info",
    message,
    action,
    dismissible = false,
    ondismiss,
    class: className,
  }: Props = $props();

  let dismissed = $state(false);

  const variantStyles: Record<Variant, string> = {
    success: "border-success/30 bg-success/10 text-success-bold",
    warning: "border-warning/30 bg-warning/10 text-warning-bold",
    error: "border-error/30 bg-error/10 text-error-bold",
    info: "border-action/30 bg-action/10 text-action-bold",
  };

  function dismiss() {
    dismissed = true;
    ondismiss?.();
  }
</script>

{#if !dismissed}
  <div
    class={cn(
      "flex items-center justify-between border-b px-4 py-2 text-sm",
      variantStyles[variant],
      className,
    )}
    role="status"
  >
    <span class="flex-1">{message}</span>
    <div class="flex items-center gap-2">
      {#if action}
        {@render action()}
      {/if}
      {#if dismissible}
        <button
          type="button"
          onclick={dismiss}
          class="ml-2 rounded-sm p-0.5 opacity-70 hover:opacity-100"
          aria-label="Dismiss"
        >
          <X class="size-4" />
        </button>
      {/if}
    </div>
  </div>
{/if}
