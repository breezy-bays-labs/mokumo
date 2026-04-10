<script lang="ts">
  import * as AlertDialog from "$lib/components/ui/alert-dialog";
  import { buttonVariants } from "$lib/components/ui/button";
  import { cn } from "$lib/utils.js";

  interface Props {
    open: boolean;
    description?: string;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let {
    open,
    description = "You have unsaved changes that will be lost if you leave this page.",
    onconfirm,
    oncancel,
  }: Props = $props();
</script>

<AlertDialog.Root
  {open}
  onOpenChange={(isOpen) => {
    if (!isOpen && open) oncancel();
  }}
>
  <AlertDialog.Content data-testid="unsaved-changes-dialog">
    <AlertDialog.Header>
      <AlertDialog.Title>Unsaved changes</AlertDialog.Title>
      <AlertDialog.Description>
        {description}
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel
        onclick={oncancel}
        data-testid="unsaved-changes-cancel-btn"
      >
        Cancel
      </AlertDialog.Cancel>
      <button
        data-slot="alert-dialog-action"
        class={cn(buttonVariants({ variant: "destructive" }))}
        onclick={onconfirm}
        data-testid="unsaved-changes-confirm-btn"
      >
        Leave anyway
      </button>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
