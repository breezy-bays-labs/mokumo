<script module>
  import { defineMeta } from "@storybook/addon-svelte-csf";
  import AppSidebar from "./app-sidebar.svelte";
  import { SidebarProvider, SidebarInset } from "$lib/components/ui/sidebar";

  const { Story } = defineMeta({
    title: "UI/AppSidebar",
    component: AppSidebar,
    tags: ["autodocs"],
  });
</script>

<Story name="Default">
  <SidebarProvider>
    <AppSidebar />
    <SidebarInset>
      <div class="p-4">
        <p class="text-muted-foreground">Page content area</p>
      </div>
    </SidebarInset>
  </SidebarProvider>
</Story>
