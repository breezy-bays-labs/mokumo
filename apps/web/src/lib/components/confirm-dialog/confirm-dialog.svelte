<script lang="ts">
  import type { Snippet } from "svelte";
  import * as AlertDialog from "$lib/components/ui/alert-dialog";
  import {
    buttonVariants,
    type ButtonVariant,
  } from "$lib/components/ui/button";
  import { cn } from "$lib/utils.js";
  import Loader2 from "@lucide/svelte/icons/loader-2";

  interface Props {
    open?: boolean;
    title: string;
    description: string;
    onConfirm: () => void | Promise<void>;
    variant?: ButtonVariant;
    confirmLabel?: string;
    cancelLabel?: string;
    children?: Snippet<[Record<string, unknown>]>;
  }

  let {
    open = $bindable(false),
    title,
    description,
    onConfirm,
    variant = "default",
    confirmLabel = "Continue",
    cancelLabel = "Cancel",
    children,
  }: Props = $props();

  let loading = $state(false);
  let error = $state<string | null>(null);

  // Clear stale error when dialog reopens
  $effect(() => {
    if (open) {
      error = null;
    }
  });

  async function handleConfirm() {
    loading = true;
    error = null;
    try {
      await onConfirm();
      open = false;
    } catch (e) {
      error = e instanceof Error ? e.message : "An error occurred";
    } finally {
      loading = false;
    }
  }
</script>

<AlertDialog.Root bind:open>
  {#if children}
    <AlertDialog.Trigger>
      {#snippet child({ props })}
        {@render children(props)}
      {/snippet}
    </AlertDialog.Trigger>
  {/if}
  <AlertDialog.Content onEscapeKeydown={(e) => e.preventDefault()}>
    <AlertDialog.Header>
      <AlertDialog.Title>{title}</AlertDialog.Title>
      <AlertDialog.Description>{description}</AlertDialog.Description>
    </AlertDialog.Header>
    {#if error}
      <div
        class="rounded-md bg-error/10 border border-error px-3 py-2 text-sm text-foreground"
      >
        {error}
      </div>
    {/if}
    <AlertDialog.Footer>
      <AlertDialog.Cancel disabled={loading}>{cancelLabel}</AlertDialog.Cancel>
      <button
        data-slot="alert-dialog-action"
        class={cn(buttonVariants({ variant }), "gap-2")}
        disabled={loading}
        onclick={handleConfirm}
      >
        {#if loading}
          <Loader2 class="h-4 w-4 animate-spin" />
        {/if}
        {confirmLabel}
      </button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
