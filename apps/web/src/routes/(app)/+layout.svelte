<script lang="ts">
  import { goto } from "$app/navigation";
  import AppSidebar from "$lib/components/app-sidebar.svelte";
  import AppTopbar from "$lib/components/app-topbar.svelte";
  import { SidebarInset, SidebarProvider } from "$lib/components/ui/sidebar";

  let { data, children } = $props();

  const STORAGE_KEY = "sidebar:state";

  let sidebarOpen = $state(
    typeof window !== "undefined"
      ? localStorage.getItem(STORAGE_KEY) !== "false"
      : true,
  );

  let ready = $state(false);

  $effect(() => {
    if (data.redirect) {
      goto(data.redirect);
    } else {
      ready = true;
    }
  });

  function handleOpenChange(open: boolean) {
    sidebarOpen = open;
    localStorage.setItem(STORAGE_KEY, String(open));
  }
</script>

{#if ready}
  <SidebarProvider open={sidebarOpen} onOpenChange={handleOpenChange}>
    <AppSidebar />
    <SidebarInset>
      <AppTopbar />
      <main class="flex-1 p-4">
        {@render children()}
      </main>
    </SidebarInset>
  </SidebarProvider>
{:else}
  <div class="flex h-screen items-center justify-center">
    <div class="text-muted-foreground text-sm">Loading...</div>
  </div>
{/if}
