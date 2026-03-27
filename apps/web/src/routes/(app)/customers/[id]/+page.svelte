<script lang="ts">
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Separator } from "$lib/components/ui/separator";
  import { getCustomerContext } from "$lib/contexts/customer-context.svelte";
  import { PAYMENT_TERMS_OPTIONS } from "$lib/schemas/customer";
  import { formatCurrency } from "$lib/utils/format";

  const ctx = getCustomerContext();

  function formatPaymentTerms(terms: string | null): string {
    if (!terms) return "—";
    return PAYMENT_TERMS_OPTIONS.find((o) => o.value === terms)?.label ?? terms;
  }
</script>

{#if ctx.customer}
  <div class="grid gap-4 md:grid-cols-2 pt-4">
    <Card>
      <CardHeader>
        <CardTitle>Contact Information</CardTitle>
      </CardHeader>
      <CardContent class="space-y-3">
        <div>
          <p class="text-sm font-medium text-muted-foreground">Display Name</p>
          <p>{ctx.customer.display_name}</p>
        </div>
        {#if ctx.customer.company_name}
          <div>
            <p class="text-sm font-medium text-muted-foreground">Company</p>
            <p>{ctx.customer.company_name}</p>
          </div>
        {/if}
        <div>
          <p class="text-sm font-medium text-muted-foreground">Email</p>
          <p>{ctx.customer.email ?? "—"}</p>
        </div>
        <div>
          <p class="text-sm font-medium text-muted-foreground">Phone</p>
          <p>{ctx.customer.phone ?? "—"}</p>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Address</CardTitle>
      </CardHeader>
      <CardContent>
        {#if ctx.customer.address_line1}
          <p>{ctx.customer.address_line1}</p>
          {#if ctx.customer.address_line2}
            <p>{ctx.customer.address_line2}</p>
          {/if}
          <p>
            {[ctx.customer.city, ctx.customer.state, ctx.customer.postal_code]
              .filter(Boolean)
              .join(", ")}
          </p>
          <p>{ctx.customer.country ?? ""}</p>
        {:else}
          <p class="text-muted-foreground">No address on file</p>
        {/if}
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Financial Defaults</CardTitle>
      </CardHeader>
      <CardContent class="space-y-3">
        <div>
          <p class="text-sm font-medium text-muted-foreground">Payment Terms</p>
          <p>{formatPaymentTerms(ctx.customer.payment_terms)}</p>
        </div>
        <Separator />
        <div>
          <p class="text-sm font-medium text-muted-foreground">Credit Limit</p>
          <p>{formatCurrency(ctx.customer.credit_limit_cents)}</p>
        </div>
        <Separator />
        <div>
          <p class="text-sm font-medium text-muted-foreground">Tax Exempt</p>
          <p>{ctx.customer.tax_exempt ? "Yes" : "No"}</p>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Notes</CardTitle>
      </CardHeader>
      <CardContent>
        {#if ctx.customer.notes}
          <p class="whitespace-pre-wrap">{ctx.customer.notes}</p>
        {:else}
          <p class="text-muted-foreground">No notes</p>
        {/if}
      </CardContent>
    </Card>
  </div>
{/if}
