<script lang="ts">
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Badge } from "$lib/components/ui/badge";
  import { mockPricingTemplates } from "$lib/mocks/customer-pricing";
</script>

<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3 pt-4">
  {#each mockPricingTemplates as template (template.id)}
    <Card>
      <CardHeader>
        <div class="flex items-center justify-between">
          <CardTitle class="text-base">{template.name}</CardTitle>
          <Badge variant="outline">{template.method}</Badge>
        </div>
      </CardHeader>
      <CardContent class="space-y-2">
        <div>
          <p class="text-sm font-medium text-muted-foreground">Base Price</p>
          <p class="text-lg font-semibold">{template.basePrice}</p>
        </div>
        <div>
          <p class="text-sm font-medium text-muted-foreground">Discount</p>
          <p class="text-sm">{template.discount}</p>
        </div>
      </CardContent>
    </Card>
  {/each}
</div>
