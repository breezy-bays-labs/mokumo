<script lang="ts">
  import SidebarIconOnly from "./sidebar-icon-only.svelte";
  import Home from "@lucide/svelte/icons/home";
  import Users from "@lucide/svelte/icons/users";
  import FileText from "@lucide/svelte/icons/file-text";
  import Settings from "@lucide/svelte/icons/settings";
  import Shirt from "@lucide/svelte/icons/shirt";
</script>

{#snippet homeIcon()}<Home class="size-5" />{/snippet}
{#snippet usersIcon()}<Users class="size-5" />{/snippet}
{#snippet shirtIcon()}<Shirt class="size-5" />{/snippet}
{#snippet fileTextIcon()}<FileText class="size-5" />{/snippet}
{#snippet settingsIcon()}<Settings class="size-5" />{/snippet}

<SidebarIconOnly
  items={[
    { label: "Dashboard", href: "/", icon: homeIcon, active: true },
    { label: "Customers", href: "/customers", icon: usersIcon },
    { label: "Garments", href: "/garments", icon: shirtIcon },
    { label: "Quotes", href: "/quotes", icon: fileTextIcon },
    { label: "Settings", href: "/settings", icon: settingsIcon },
  ]}
/>
