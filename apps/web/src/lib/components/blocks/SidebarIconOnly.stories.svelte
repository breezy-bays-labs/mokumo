<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import SidebarIconOnly from "./sidebar-icon-only.svelte";

  const { Story } = defineMeta({
    title: "Blocks/SidebarIconOnly",
    component: SidebarIconOnly,
    tags: ["autodocs"],
  });
</script>

<script>
  import SidebarIconOnlyDemo from "./sidebar-icon-only-demo.svelte";
</script>

<Story name="Default">
  <div class="h-[400px]">
    <SidebarIconOnlyDemo />
  </div>
</Story>
