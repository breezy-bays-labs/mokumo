<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import { Button } from "$lib/components/ui/button";

  interface Props {
    open: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let { open, onconfirm, oncancel }: Props = $props();
</script>

<Dialog.Root
  {open}
  onOpenChange={(isOpen) => {
    if (!isOpen && open) oncancel();
  }}
>
  <Dialog.Content data-testid="unsaved-changes-dialog" showCloseButton={false}>
    <Dialog.Header>
      <Dialog.Title>Unsaved changes</Dialog.Title>
      <Dialog.Description>
        You have unsaved changes that will be lost if you switch profiles.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button
        variant="outline"
        onclick={oncancel}
        data-testid="unsaved-changes-cancel-btn"
      >
        Cancel
      </Button>
      <Button
        variant="destructive"
        onclick={onconfirm}
        data-testid="unsaved-changes-confirm-btn"
      >
        Leave anyway
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
