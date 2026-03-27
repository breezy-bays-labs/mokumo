<script lang="ts">
  import { Card, CardContent } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { mockArtwork } from "$lib/mocks/customer-artwork";
  import Image from "@lucide/svelte/icons/image";
</script>

<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4 pt-4">
  {#each mockArtwork as art (art.id)}
    <Card class="overflow-hidden">
      <div class="flex h-32 items-center justify-center bg-muted">
        <Image class="h-10 w-10 text-muted-foreground" />
      </div>
      <CardContent class="p-3 space-y-1">
        <p class="text-sm font-medium truncate">{art.name}</p>
        <div class="flex items-center justify-between">
          <Badge variant="outline" class="text-xs">{art.type}</Badge>
          <span class="text-xs text-muted-foreground">{art.dimensions}</span>
        </div>
        <p class="text-xs text-muted-foreground">
          Uploaded {new Date(art.uploadedAt).toLocaleDateString()}
        </p>
      </CardContent>
    </Card>
  {/each}
</div>
