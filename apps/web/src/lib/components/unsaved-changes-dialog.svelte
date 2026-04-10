<script lang="ts">
  import * as AlertDialog from "$lib/components/ui/alert-dialog";
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
      <AlertDialog.Cancel data-testid="unsaved-changes-cancel-btn">
        Cancel
      </AlertDialog.Cancel>
      <AlertDialog.Action
        variant="destructive"
        onclick={onconfirm}
        data-testid="unsaved-changes-confirm-btn"
      >
        Leave anyway
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
