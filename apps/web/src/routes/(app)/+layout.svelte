<script lang="ts">
  import AppSidebar from "$lib/components/app-sidebar.svelte";
  import AppTopbar from "$lib/components/app-topbar.svelte";
  import { SidebarInset, SidebarProvider } from "$lib/components/ui/sidebar";

  let { children } = $props();

  const STORAGE_KEY = "sidebar:state";

  let sidebarOpen = $state(
    typeof window !== "undefined"
      ? localStorage.getItem(STORAGE_KEY) !== "false"
      : true,
  );

  function handleOpenChange(open: boolean) {
    sidebarOpen = open;
    localStorage.setItem(STORAGE_KEY, String(open));
  }
</script>

<SidebarProvider open={sidebarOpen} onOpenChange={handleOpenChange}>
  <AppSidebar />
  <SidebarInset>
    <AppTopbar />
    <main class="flex-1 p-4">
      {@render children()}
    </main>
  </SidebarInset>
</SidebarProvider>
