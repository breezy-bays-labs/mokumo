<script lang="ts">
  import { Menubar as MenubarPrimitive } from "bits-ui";

  let { ref = $bindable(null), ...restProps }: MenubarPrimitive.GroupProps =
    $props();
</script>

<MenubarPrimitive.Group bind:ref data-slot="menubar-group" {...restProps} />
