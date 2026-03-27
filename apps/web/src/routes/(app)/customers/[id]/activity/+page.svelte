<script lang="ts">
  import { page as pageState } from "$app/state";
  import { goto } from "$app/navigation";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { Skeleton } from "$lib/components/ui/skeleton";
  import type { ActivityEntryResponse } from "$lib/types/ActivityEntryResponse";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  let { data } = $props();

  function actionVariant(
    action: string,
  ): "default" | "secondary" | "destructive" | "outline" {
    switch (action) {
      case "created":
        return "default";
      case "updated":
        return "secondary";
      case "soft_deleted":
        return "destructive";
      default:
        return "outline";
    }
  }

  function actionLabel(action: string): string {
    switch (action) {
      case "created":
        return "Created";
      case "updated":
        return "Updated";
      case "soft_deleted":
        return "Archived";
      default:
        return action;
    }
  }

  function formatTimestamp(ts: string): string {
    return new Date(ts).toLocaleString();
  }

  function describeAction(entry: ActivityEntryResponse): string | null {
    switch (entry.action) {
      case "created":
        return "Customer record created";
      case "updated":
        return "Customer record updated";
      case "soft_deleted":
        return "Customer archived";
      default:
        return null;
    }
  }

  function navigatePage(newPage: number) {
    const url = new URL(pageState.url);
    url.searchParams.set("page", String(newPage));
    goto(url.toString());
  }
</script>

<div class="space-y-4 pt-4">
  {#if data.error}
    <div class="bg-destructive/10 text-destructive rounded-lg p-4">
      <p>{data.error}</p>
    </div>
  {:else if !data.activity}
    <div class="space-y-3">
      {#each Array(4) as _}
        <div class="flex items-start gap-3 rounded-lg border p-4">
          <Skeleton class="h-6 w-20" />
          <div class="flex-1 space-y-1">
            <Skeleton class="h-4 w-48" />
            <Skeleton class="h-3 w-32" />
          </div>
        </div>
      {/each}
    </div>
  {:else if data.activity.items.length === 0}
    <p class="text-muted-foreground text-center py-8">
      No activity recorded yet.
    </p>
  {:else}
    <div class="space-y-3">
      {#each data.activity.items as entry (entry.id)}
        {@const description = describeAction(entry)}
        <div class="flex items-start gap-3 rounded-lg border p-4">
          <Badge variant={actionVariant(entry.action)}>
            {actionLabel(entry.action)}
          </Badge>
          <div class="flex-1 min-w-0">
            <p class="text-sm font-medium">
              {description ??
                `${actionLabel(entry.action)} by ${entry.actor_type}`}
            </p>
            <p class="text-xs text-muted-foreground mt-1">
              {formatTimestamp(entry.created_at)}
            </p>
          </div>
        </div>
      {/each}
    </div>

    {#if data.activity.total_pages > 1}
      <div class="flex items-center justify-center gap-2 pt-2">
        <Button
          variant="outline"
          size="sm"
          disabled={data.activity.page <= 1}
          onclick={() => navigatePage(data.activity!.page - 1)}
        >
          <ChevronLeft class="h-4 w-4" />
          Previous
        </Button>
        <span class="text-sm text-muted-foreground">
          Page {data.activity.page} of {data.activity.total_pages}
        </span>
        <Button
          variant="outline"
          size="sm"
          disabled={data.activity.page >= data.activity.total_pages}
          onclick={() => navigatePage(data.activity!.page + 1)}
        >
          Next
          <ChevronRight class="h-4 w-4" />
        </Button>
      </div>
    {/if}
  {/if}
</div>
