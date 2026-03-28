<script lang="ts">
  import type { Snippet } from "svelte";
  import AppSidebar from "$lib/components/app-sidebar.svelte";
  import AppTopbar from "$lib/components/app-topbar.svelte";
  import RecoveryCodeWarning from "$lib/components/recovery-code-warning.svelte";
  import { SidebarInset, SidebarProvider } from "$lib/components/ui/sidebar";
  import type { LayoutData } from "./$types";

  let { children, data }: { children: Snippet; data: LayoutData } = $props();

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
    <RecoveryCodeWarning count={data.recovery_codes_remaining} />
    <main class="flex-1 p-4">
      {@render children()}
    </main>
  </SidebarInset>
</SidebarProvider>
