<script lang="ts">
  import * as AlertDialog from "$lib/components/ui/alert-dialog";
  import { buttonVariants } from "$lib/components/ui/button";
  import { cn } from "$lib/utils.js";
  import Loader2 from "@lucide/svelte/icons/loader-2";

  interface Props {
    open?: boolean;
    title: string;
    description: string;
    onConfirm: (password: string) => Promise<void>;
  }

  let {
    open = $bindable(false),
    title,
    description,
    onConfirm,
  }: Props = $props();

  let password = $state("");
  let loading = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (open) {
      password = "";
      error = null;
    }
  });

  async function handleConfirm() {
    loading = true;
    error = null;
    try {
      await onConfirm(password);
      open = false;
    } catch (e) {
      error = e instanceof Error ? e.message : "An error occurred";
    } finally {
      loading = false;
    }
  }
</script>

<AlertDialog.Root bind:open>
  <AlertDialog.Content onEscapeKeydown={(e) => e.preventDefault()}>
    <AlertDialog.Header>
      <AlertDialog.Title>{title}</AlertDialog.Title>
      <AlertDialog.Description>{description}</AlertDialog.Description>
    </AlertDialog.Header>
    <div class="space-y-2">
      <label for="regen-password" class="text-sm font-medium">
        Current Password
      </label>
      <input
        id="regen-password"
        type="password"
        bind:value={password}
        class="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        placeholder="Enter your current password"
        disabled={loading}
      />
    </div>
    {#if error}
      <div
        class="rounded-md bg-error/10 border border-error px-3 py-2 text-sm text-foreground"
      >
        {error}
      </div>
    {/if}
    <AlertDialog.Footer>
      <AlertDialog.Cancel disabled={loading}>Cancel</AlertDialog.Cancel>
      <button
        data-slot="alert-dialog-action"
        class={cn(buttonVariants({ variant: "destructive" }), "gap-2")}
        disabled={loading || !password}
        onclick={handleConfirm}
      >
        {#if loading}
          <Loader2 class="h-4 w-4 animate-spin" />
        {/if}
        Regenerate
      </button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
