<script lang="ts">
  import { Badge } from "$lib/components/ui/badge";
  import { mockMessages } from "$lib/mocks/customer-communication";
  import ArrowDownLeft from "@lucide/svelte/icons/arrow-down-left";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
</script>

<div class="space-y-3 pt-4">
  {#each mockMessages as message (message.id)}
    <div class="flex items-start gap-3 rounded-lg border p-4">
      <div class="mt-0.5">
        {#if message.direction === "inbound"}
          <ArrowDownLeft class="h-4 w-4 text-blue-500" />
        {:else}
          <ArrowUpRight class="h-4 w-4 text-green-500" />
        {/if}
      </div>
      <div class="flex-1 min-w-0">
        <div class="flex items-center gap-2">
          <p class="text-sm font-medium truncate">{message.subject}</p>
          <Badge variant="outline" class="text-xs shrink-0"
            >{message.channel}</Badge
          >
        </div>
        <p class="text-sm text-muted-foreground mt-0.5 truncate">
          {message.preview}
        </p>
        <p class="text-xs text-muted-foreground mt-1">
          {new Date(message.timestamp).toLocaleString()}
        </p>
      </div>
    </div>
  {/each}
</div>
