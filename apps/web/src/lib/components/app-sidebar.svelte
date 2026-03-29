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
  import { mode, setMode } from "mode-watcher";
  import { toast } from "$lib/components/toast";
  import { apiFetch } from "$lib/api";
  import { DEMO_GUIDE_URL } from "$lib/config/constants";
  import CircleHelp from "@lucide/svelte/icons/circle-help";
  import LogOut from "@lucide/svelte/icons/log-out";
  import Moon from "@lucide/svelte/icons/moon";
  import Sun from "@lucide/svelte/icons/sun";
  import UserRound from "@lucide/svelte/icons/user-round";

  const visibleItems = navItems.filter((item) => !item.hidden);

  const themes = [
    { value: "niji", label: "Niji", swatch: "oklch(0.56 0.158 249.8)" },
    {
      value: "tangerine",
      label: "Tangerine",
      swatch: "oklch(0.6397 0.172 36.4421)",
    },
    {
      value: "midnight-bloom",
      label: "Midnight",
      swatch: "oklch(0.5676 0.2021 283.0838)",
    },
    {
      value: "solar-dusk",
      label: "Solar",
      swatch: "oklch(0.5553 0.1455 48.9975)",
    },
    {
      value: "soft-pop",
      label: "Soft Pop",
      swatch: "oklch(0.5106 0.2301 276.9656)",
    },
    {
      value: "sunset-horizon",
      label: "Sunset",
      swatch: "oklch(0.7357 0.1641 34.7091)",
    },
  ] as const;

  const THEME_CLASSES = [
    "theme-tangerine",
    "theme-midnight-bloom",
    "theme-solar-dusk",
    "theme-soft-pop",
    "theme-sunset-horizon",
  ];

  let activeTheme = $state(
    (typeof localStorage !== "undefined" &&
      localStorage.getItem("mokumo-theme")) ||
      "niji",
  );

  function applyTheme(value: string) {
    activeTheme = value;
    const root = document.documentElement;
    for (const cls of THEME_CLASSES) {
      root.classList.remove(cls);
    }
    if (value !== "niji") {
      root.classList.add(`theme-${value}`);
    }
    localStorage.setItem("mokumo-theme", value);
  }

  // Apply saved theme on mount
  $effect(() => {
    applyTheme(activeTheme);
  });

  function toggleMode() {
    setMode(mode.current === "dark" ? "light" : "dark");
  }

  let loggingOut = $state(false);

  async function handleLogout() {
    if (loggingOut) return;
    loggingOut = true;
    const result = await apiFetch("/api/auth/logout", { method: "POST" });
    if (!result.ok) {
      loggingOut = false;
      console.error("Logout failed:", result.status, result.error);
      toast.error("Logout failed. Please try again.");
      return;
    }
    try {
      await goto("/login");
    } catch (error) {
      console.error("Logout navigation failed:", error);
      window.location.assign("/login");
    }
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
          <Popover.Trigger>
            <Sidebar.SidebarMenuButton
              tooltipContent="Help"
              data-testid="help-trigger"
            >
              <CircleHelp class="size-4" />
              <span class="group-data-[collapsible=icon]:hidden">Help</span>
            </Sidebar.SidebarMenuButton>
          </Popover.Trigger>
          <Popover.Content
            side="top"
            align="start"
            class="w-64 p-4"
            data-testid="help-popover"
          >
            <h3 class="text-sm font-semibold">Demo Guide</h3>
            <p class="mt-1 text-xs text-muted-foreground">
              Step-by-step walkthrough for setting up and exploring your shop.
            </p>
            <a
              href={DEMO_GUIDE_URL}
              target="_blank"
              rel="noopener noreferrer"
              class="mt-3 inline-flex w-full items-center justify-center rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
              data-testid="open-demo-guide"
            >
              Open Demo Guide
            </a>
            <p
              class="mt-2 text-xs text-muted-foreground"
              data-testid="internet-note"
            >
              Requires internet connection
            </p>
          </Popover.Content>
        </Popover.Root>
      </Sidebar.SidebarMenuItem>
      <Sidebar.SidebarMenuItem>
        <Popover.Root>
          <Popover.Trigger
            class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 hover:bg-sidebar-accent"
            data-testid="user-menu-trigger"
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
            <div class="space-y-2">
              <p class="px-2 py-1 text-xs font-medium text-muted-foreground">
                Mode
              </p>
              <button
                onclick={toggleMode}
                class="mx-2 flex h-8 w-full max-w-[calc(100%-1rem)] items-center rounded-full bg-muted p-0.5"
                aria-label="Toggle light/dark mode"
              >
                <span
                  class="flex h-7 w-1/2 items-center justify-center rounded-full transition-colors {mode.current !==
                  'dark'
                    ? 'bg-background shadow-sm'
                    : ''}"
                >
                  <Sun class="size-4" />
                </span>
                <span
                  class="flex h-7 w-1/2 items-center justify-center rounded-full transition-colors {mode.current ===
                  'dark'
                    ? 'bg-background shadow-sm'
                    : ''}"
                >
                  <Moon class="size-4" />
                </span>
              </button>
            </div>
            <div class="mt-2 space-y-2">
              <p class="px-2 py-1 text-xs font-medium text-muted-foreground">
                Theme
              </p>
              <div class="grid grid-cols-3 gap-1.5 px-2">
                {#each themes as theme (theme.value)}
                  <button
                    onclick={() => applyTheme(theme.value)}
                    class="flex flex-col items-center gap-1 rounded-md p-1.5 text-xs transition-colors {activeTheme ===
                    theme.value
                      ? 'bg-accent font-medium'
                      : 'hover:bg-accent/50'}"
                    title={theme.label}
                  >
                    <span
                      class="size-6 rounded-full border-2 transition-transform {activeTheme ===
                      theme.value
                        ? 'border-primary scale-110'
                        : 'border-transparent'}"
                      style="background-color: {theme.swatch}"
                    ></span>
                    <span class="truncate max-w-full">{theme.label}</span>
                  </button>
                {/each}
              </div>
            </div>
            <div class="my-1 h-px bg-border"></div>
            <button
              onclick={handleLogout}
              disabled={loggingOut}
              class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-accent disabled:opacity-50"
              data-testid="logout-button"
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
