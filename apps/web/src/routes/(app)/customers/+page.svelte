<script lang="ts">
  import { goto, invalidate } from "$app/navigation";
  import { deleteCustomer } from "$lib/api/customers";
  import ConfirmDialog from "$lib/components/confirm-dialog/confirm-dialog.svelte";
  import CustomerFormSheet from "$lib/components/customer-form-sheet.svelte";
  import EmptyState from "$lib/components/empty-state.svelte";
  import { toast } from "$lib/components/toast";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
  } from "$lib/components/ui/dropdown-menu";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import {
    Pagination,
    PaginationContent,
    PaginationEllipsis,
    PaginationItem,
    PaginationLink,
  } from "$lib/components/ui/pagination";
  import { Skeleton } from "$lib/components/ui/skeleton";
  import { Switch } from "$lib/components/ui/switch";
  import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
  } from "$lib/components/ui/table";
  import { useSearchParams } from "$lib/hooks/use-url-params.svelte";
  import { customerListParamsSchema } from "$lib/schemas/customer";
  import type { CustomerResponse } from "$lib/types/CustomerResponse";
  import type { PaginatedList } from "$lib/types/PaginatedList";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import Users from "@lucide/svelte/icons/users";

  let { data } = $props();

  const params = useSearchParams(customerListParamsSchema, {
    debounce: 300,
    pushHistory: false,
    noScroll: true,
  });

  let formSheetOpen = $state(false);
  let archiveDialogOpen = $state(false);
  let archiveTarget = $state<CustomerResponse | null>(null);

  let customers = $derived(
    data.customers as PaginatedList<CustomerResponse> | null,
  );
  let error = $derived(data.error as string | null);
  let hasArchivedCustomers = $derived(data.hasArchivedCustomers as boolean);
  let isLoading = $derived(!customers && !error);
  let isEmpty = $derived(
    customers?.total === 0 &&
      !params.search &&
      !params.include_deleted &&
      !hasArchivedCustomers,
  );
  let hasOnlyArchived = $derived(
    customers?.total === 0 &&
      !params.search &&
      !params.include_deleted &&
      hasArchivedCustomers,
  );
  let isFilteredEmpty = $derived(
    customers?.total === 0 && (!!params.search || params.include_deleted),
  );

  function handleRowClick(id: string) {
    goto(`/customers/${id}`);
  }

  function handleAddCustomer() {
    formSheetOpen = true;
  }

  function openArchiveDialog(customer: CustomerResponse) {
    archiveTarget = customer;
    archiveDialogOpen = true;
  }

  async function handleArchiveConfirm() {
    if (!archiveTarget) return;
    const result = await deleteCustomer(archiveTarget.id);
    if (result.ok) {
      toast.success(`"${archiveTarget.display_name}" archived`);
      archiveDialogOpen = false;
      archiveTarget = null;
      await invalidate("app:customers");
    } else {
      throw new Error(result.error.message);
    }
  }
</script>

{#snippet showArchivedToggle(id: string)}
  <div class="flex items-center gap-2">
    <Switch
      {id}
      checked={params.include_deleted}
      onCheckedChange={(checked) => {
        params.include_deleted = checked;
        params.page = 1;
      }}
    />
    <Label for={id} class="text-sm">Show archived</Label>
  </div>
{/snippet}

{#if error}
  <div class="flex flex-col items-center justify-center py-24 text-center">
    <div class="bg-destructive/10 text-destructive rounded-lg p-6 max-w-md">
      <h2 class="text-lg font-semibold">Could not load customers</h2>
      <p class="mt-2 text-sm">{error}</p>
      <Button
        variant="outline"
        class="mt-4"
        onclick={() => invalidate("app:customers")}
      >
        Try again
      </Button>
    </div>
  </div>
{:else if isLoading}
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <Skeleton class="h-8 w-32" />
      <Skeleton class="h-10 w-36" />
    </div>
    <div class="flex items-center gap-4">
      <Skeleton class="h-10 w-64" />
      <Skeleton class="h-6 w-32" />
    </div>
    <div class="rounded-md border">
      {#each Array(6) as _}
        <div class="flex items-center gap-4 border-b p-4">
          <Skeleton class="h-4 w-48" />
          <Skeleton class="h-4 w-32" />
          <Skeleton class="h-4 w-40" />
          <Skeleton class="h-4 w-28" />
        </div>
      {/each}
    </div>
  </div>
{:else if isEmpty}
  <EmptyState
    icon={Users}
    title="No customers yet"
    subtitle="Add your first customer to get started."
  />
  <div class="flex justify-center -mt-12">
    <Button onclick={handleAddCustomer}>Add Customer</Button>
  </div>
{:else if hasOnlyArchived}
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">Customers</h1>
        <p class="text-sm text-muted-foreground">All customers are archived</p>
      </div>
      <Button onclick={handleAddCustomer}>Add Customer</Button>
    </div>
    {@render showArchivedToggle("show-deleted-empty")}
  </div>
{:else if customers}
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight">Customers</h1>
        <p class="text-sm text-muted-foreground">
          {customers.total} total customer{customers.total !== 1 ? "s" : ""}
        </p>
      </div>
      <Button onclick={handleAddCustomer}>Add Customer</Button>
    </div>

    <div class="flex items-center gap-4">
      <Input
        type="search"
        placeholder="Search customers…"
        class="max-w-xs"
        value={params.search}
        oninput={(e) => {
          params.search = e.currentTarget.value;
          params.page = 1;
        }}
      />
      {@render showArchivedToggle("show-deleted")}
    </div>

    {#if isFilteredEmpty}
      <div class="flex flex-col items-center justify-center py-16 text-center">
        <p class="text-muted-foreground">No customers match your filters.</p>
        <Button
          variant="link"
          class="mt-2"
          onclick={() => {
            params.search = "";
            params.include_deleted = false;
            params.page = 1;
          }}
        >
          Clear filters
        </Button>
      </div>
    {:else}
      <div class="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Company</TableHead>
              <TableHead>Email</TableHead>
              <TableHead>Phone</TableHead>
              <TableHead class="w-10"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {#each customers.items as customer (customer.id)}
              <TableRow
                class="cursor-pointer {customer.deleted_at ? 'opacity-50' : ''}"
                onclick={(e) => {
                  const target = e.target as HTMLElement;
                  if (
                    target.closest('[data-slot="dropdown-menu-trigger"]') ||
                    target.closest('[role="menu"]')
                  )
                    return;
                  handleRowClick(customer.id);
                }}
              >
                <TableCell class="font-medium">
                  {customer.display_name}
                  {#if customer.deleted_at}
                    <Badge variant="destructive" class="ml-2 text-xs"
                      >Archived</Badge
                    >
                  {/if}
                </TableCell>
                <TableCell>{customer.company_name ?? "—"}</TableCell>
                <TableCell>{customer.email ?? "—"}</TableCell>
                <TableCell>{customer.phone ?? "—"}</TableCell>
                <TableCell>
                  <DropdownMenu>
                    <DropdownMenuTrigger>
                      {#snippet child({ props })}
                        <Button
                          variant="ghost"
                          size="icon"
                          class="h-8 w-8"
                          {...props}
                        >
                          <EllipsisIcon class="h-4 w-4" />
                          <span class="sr-only">Actions</span>
                        </Button>
                      {/snippet}
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        onclick={(e) => {
                          e.stopPropagation();
                          goto(`/customers/${customer.id}`);
                        }}
                      >
                        View
                      </DropdownMenuItem>
                      {#if !customer.deleted_at}
                        <DropdownMenuItem
                          class="text-destructive"
                          onclick={(e) => {
                            e.stopPropagation();
                            openArchiveDialog(customer);
                          }}
                        >
                          Archive
                        </DropdownMenuItem>
                      {/if}
                    </DropdownMenuContent>
                  </DropdownMenu>
                </TableCell>
              </TableRow>
            {/each}
          </TableBody>
        </Table>
      </div>

      {#if customers.total_pages > 1}
        <div class="flex justify-center">
          <Pagination
            count={customers.total}
            perPage={customers.per_page}
            page={params.page}
            onPageChange={(p) => {
              params.page = p;
            }}
          >
            {#snippet children({ pages, currentPage })}
              <PaginationContent>
                <PaginationItem>
                  <Button
                    variant="ghost"
                    size="default"
                    class="gap-1 pl-2.5"
                    disabled={currentPage <= 1}
                    onclick={() => {
                      params.page = currentPage - 1;
                    }}
                  >
                    <ChevronLeft class="h-4 w-4" />
                    <span class="hidden sm:block">Previous</span>
                  </Button>
                </PaginationItem>
                {#each pages as page (page.key)}
                  {#if page.type === "ellipsis"}
                    <PaginationItem>
                      <PaginationEllipsis />
                    </PaginationItem>
                  {:else}
                    <PaginationItem>
                      <PaginationLink
                        {page}
                        isActive={currentPage === page.value}
                        onclick={() => {
                          params.page = page.value;
                        }}
                      >
                        {page.value}
                      </PaginationLink>
                    </PaginationItem>
                  {/if}
                {/each}
                <PaginationItem>
                  <Button
                    variant="ghost"
                    size="default"
                    class="gap-1 pr-2.5"
                    disabled={currentPage >= (customers?.total_pages ?? 1)}
                    onclick={() => {
                      params.page = currentPage + 1;
                    }}
                  >
                    <span class="hidden sm:block">Next</span>
                    <ChevronRight class="h-4 w-4" />
                  </Button>
                </PaginationItem>
              </PaginationContent>
            {/snippet}
          </Pagination>
        </div>
      {/if}
    {/if}
  </div>
{/if}

<CustomerFormSheet
  bind:open={formSheetOpen}
  onClose={() => {
    formSheetOpen = false;
  }}
/>

<ConfirmDialog
  bind:open={archiveDialogOpen}
  title="Archive customer"
  description="Are you sure you want to archive {archiveTarget?.display_name}? This action can be undone later."
  variant="destructive"
  confirmLabel="Archive"
  onConfirm={handleArchiveConfirm}
/>
