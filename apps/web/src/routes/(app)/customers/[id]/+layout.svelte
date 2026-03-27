<script lang="ts">
  import { goto, invalidate } from "$app/navigation";
  import { deleteCustomer } from "$lib/api/customers";
  import ConfirmDialog from "$lib/components/confirm-dialog/confirm-dialog.svelte";
  import CustomerFormSheet from "$lib/components/customer-form-sheet.svelte";
  import TabNav from "$lib/components/tab-nav.svelte";
  import { toast } from "$lib/components/toast";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { Skeleton } from "$lib/components/ui/skeleton";
  import {
    CustomerContext,
    setCustomerContext,
  } from "$lib/contexts/customer-context.svelte";
  import {
    clearBreadcrumbLabel,
    setBreadcrumbLabel,
  } from "$lib/config/breadcrumb-overrides.svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";

  let { data, children } = $props();

  const ctx = new CustomerContext();
  setCustomerContext(ctx);

  $effect(() => {
    ctx.customer = data.customer;
    ctx.error = data.error;
    ctx.loading = false;
    if (data.customer) {
      setBreadcrumbLabel(data.customer.id, data.customer.display_name);
    }
    return () => {
      if (data.customer) {
        clearBreadcrumbLabel(data.customer.id);
      }
    };
  });

  let tabs = $derived(
    data.customer
      ? [
          { label: "Overview", href: `/customers/${data.customer.id}` },
          {
            label: "Activity",
            href: `/customers/${data.customer.id}/activity`,
          },
          {
            label: "Contacts",
            href: `/customers/${data.customer.id}/contacts`,
          },
          { label: "Artwork", href: `/customers/${data.customer.id}/artwork` },
          { label: "Pricing", href: `/customers/${data.customer.id}/pricing` },
          {
            label: "Communication",
            href: `/customers/${data.customer.id}/communication`,
          },
        ]
      : [],
  );

  let editSheetOpen = $state(false);
  let archiveDialogOpen = $state(false);

  function handleEdit() {
    editSheetOpen = true;
  }

  function handleArchiveClick() {
    archiveDialogOpen = true;
  }

  async function handleArchiveConfirm() {
    if (!ctx.customer) return;
    const result = await deleteCustomer(ctx.customer.id);
    if (result.ok) {
      toast.success(`"${ctx.customer.display_name}" archived`);
      archiveDialogOpen = false;
      goto("/customers");
    } else {
      throw new Error(result.error.message);
    }
  }
</script>

{#if ctx.loading}
  <div class="space-y-4">
    <Skeleton class="h-8 w-64" />
    <Skeleton class="h-4 w-48" />
    <Skeleton class="h-10 w-full" />
    <Skeleton class="h-64 w-full" />
  </div>
{:else if ctx.error || !ctx.customer}
  <div class="flex flex-col items-center justify-center py-24 text-center">
    <div class="bg-destructive/10 text-destructive rounded-lg p-6 max-w-md">
      <h2 class="text-lg font-semibold">Customer not found</h2>
      <p class="mt-2 text-sm">
        {ctx.error ?? "The customer could not be loaded."}
      </p>
      <Button variant="outline" class="mt-4" onclick={() => goto("/customers")}>
        Back to customers
      </Button>
    </div>
  </div>
{:else}
  <div class="space-y-4">
    <Button
      variant="ghost"
      size="sm"
      class="gap-1"
      onclick={() => goto("/customers")}
    >
      <ArrowLeft class="h-4 w-4" />
      Customers
    </Button>

    <div class="flex items-start justify-between">
      <div>
        <div class="flex items-center gap-3">
          <h1 class="text-2xl font-semibold tracking-tight">
            {ctx.customer.display_name}
          </h1>
          {#if ctx.isArchived}
            <Badge variant="destructive">Archived</Badge>
          {/if}
        </div>
        {#if ctx.customer.company_name}
          <p class="text-muted-foreground">{ctx.customer.company_name}</p>
        {/if}
        <div class="mt-1 flex items-center gap-4 text-sm text-muted-foreground">
          {#if ctx.customer.email}
            <span>{ctx.customer.email}</span>
          {/if}
          {#if ctx.customer.phone}
            <span>{ctx.customer.phone}</span>
          {/if}
        </div>
      </div>
      <div class="flex items-center gap-2">
        {#if !ctx.isArchived}
          <Button variant="outline" onclick={handleEdit}>Edit</Button>
          <Button variant="destructive" onclick={handleArchiveClick}
            >Archive</Button
          >
        {/if}
      </div>
    </div>

    <TabNav {tabs} />

    {@render children()}
  </div>

  <CustomerFormSheet
    bind:open={editSheetOpen}
    customer={ctx.customer}
    onClose={() => {
      editSheetOpen = false;
    }}
  />

  <ConfirmDialog
    bind:open={archiveDialogOpen}
    title="Archive customer"
    description="Are you sure you want to archive {ctx.customer
      .display_name}? This action can be undone later."
    variant="destructive"
    confirmLabel="Archive"
    onConfirm={handleArchiveConfirm}
  />
{/if}
