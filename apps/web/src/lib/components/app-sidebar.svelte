<script lang="ts">
  import { untrack } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { navItems } from "$lib/config/nav-items";
  import { isActive } from "$lib/config/nav-utils";
  import * as Avatar from "$lib/components/ui/avatar";
  import * as Popover from "$lib/components/ui/popover";
  import * as Sidebar from "$lib/components/ui/sidebar";
  import { useSidebar } from "$lib/components/ui/sidebar";
  import { setMode, userPrefersMode } from "mode-watcher";
  import LogOut from "@lucide/svelte/icons/log-out";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Moon from "@lucide/svelte/icons/moon";
  import Sun from "@lucide/svelte/icons/sun";
  import UserRound from "@lucide/svelte/icons/user-round";

  const visibleItems = navItems.filter((item) => !item.hidden);

  const themeOptions = [
    { label: "Light", value: "light", icon: Sun },
    { label: "Dark", value: "dark", icon: Moon },
    { label: "System", value: "system", icon: Monitor },
  ] as const;

  function handleLogout() {
    goto("/");
  }

  const sidebar = useSidebar();

  $effect(() => {
    void page.url.pathname;
    untrack(() => {
      if (sidebar.isMobile && sidebar.openMobile) {
        sidebar.setOpenMobile(false);
      }
    });
  });
</script>

<Sidebar.Sidebar variant="sidebar" collapsible="icon">
  <Sidebar.SidebarContent>
    <Sidebar.SidebarGroup>
      <Sidebar.SidebarMenu>
        {#each visibleItems as item (item.url)}
          <Sidebar.SidebarMenuItem>
            <Sidebar.SidebarMenuButton
              isActive={isActive(item.url, page.url.pathname)}
              tooltipContent={item.title}
            >
              {#snippet child({ props })}
                <a href={item.url} {...props}>
                  <item.icon />
                  <span>{item.title}</span>
                </a>
              {/snippet}
            </Sidebar.SidebarMenuButton>
          </Sidebar.SidebarMenuItem>
        {/each}
      </Sidebar.SidebarMenu>
    </Sidebar.SidebarGroup>
  </Sidebar.SidebarContent>
  <Sidebar.SidebarFooter>
    <Sidebar.SidebarMenu>
      <Sidebar.SidebarMenuItem>
        <Popover.Root>
          <Popover.Trigger
            class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 hover:bg-sidebar-accent"
          >
            <Avatar.Avatar class="size-6">
              <Avatar.AvatarFallback>
                <UserRound class="size-4" />
              </Avatar.AvatarFallback>
            </Avatar.Avatar>
            <span
              class="text-sm font-medium group-data-[collapsible=icon]:hidden"
              >Owner</span
            >
          </Popover.Trigger>
          <Popover.Content side="top" align="start" class="w-56 p-2">
            <div class="space-y-1">
              <p class="px-2 py-1 text-xs font-medium text-muted-foreground">
                Theme
              </p>
              {#each themeOptions as option (option.value)}
                <button
                  onclick={() => setMode(option.value)}
                  class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-accent {userPrefersMode.current ===
                  option.value
                    ? 'bg-accent font-medium'
                    : ''}"
                >
                  <option.icon class="size-4" />
                  {option.label}
                </button>
              {/each}
            </div>
            <div class="my-1 h-px bg-border"></div>
            <button
              onclick={handleLogout}
              class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-accent"
            >
              <LogOut class="size-4" />
              Log out
            </button>
          </Popover.Content>
        </Popover.Root>
      </Sidebar.SidebarMenuItem>
    </Sidebar.SidebarMenu>
  </Sidebar.SidebarFooter>
  <Sidebar.SidebarRail />
</Sidebar.Sidebar>
