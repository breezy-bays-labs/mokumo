<script lang="ts">
  import { page } from "$app/state";
  import { buildBreadcrumbs } from "$lib/config/nav-utils";
  import * as Avatar from "$lib/components/ui/avatar";
  import * as Breadcrumb from "$lib/components/ui/breadcrumb";
  import { Separator } from "$lib/components/ui/separator";
  import { SidebarTrigger } from "$lib/components/ui/sidebar";
  import UserRound from "@lucide/svelte/icons/user-round";

  const segments = $derived(buildBreadcrumbs(page.url.pathname));
</script>

<header class="flex h-14 items-center gap-2 border-b px-4">
  <SidebarTrigger />
  <Separator orientation="vertical" class="mr-2 h-4" />
  <Breadcrumb.Breadcrumb>
    <Breadcrumb.BreadcrumbList>
      {#each segments as segment, i (segment.href)}
        {#if i > 0}
          <Breadcrumb.BreadcrumbSeparator />
        {/if}
        <Breadcrumb.BreadcrumbItem>
          {#if i === segments.length - 1}
            <Breadcrumb.BreadcrumbPage
              >{segment.label}</Breadcrumb.BreadcrumbPage
            >
          {:else}
            <Breadcrumb.BreadcrumbLink href={segment.href}>
              {segment.label}
            </Breadcrumb.BreadcrumbLink>
          {/if}
        </Breadcrumb.BreadcrumbItem>
      {/each}
    </Breadcrumb.BreadcrumbList>
  </Breadcrumb.Breadcrumb>
  <div class="ml-auto">
    <Avatar.Avatar class="size-8">
      <Avatar.AvatarFallback>
        <UserRound class="size-4" />
      </Avatar.AvatarFallback>
    </Avatar.Avatar>
  </div>
</header>
