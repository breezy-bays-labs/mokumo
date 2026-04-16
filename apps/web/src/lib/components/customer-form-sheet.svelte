<script lang="ts">
  import { invalidate } from "$app/navigation";
  import { createCustomer, updateCustomer } from "$lib/api/customers";
  import { toast } from "$lib/components/toast";
  import { Button } from "$lib/components/ui/button";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
  } from "$lib/components/ui/select";
  import {
    Sheet,
    SheetContent,
    SheetDescription,
    SheetFooter,
    SheetHeader,
    SheetTitle,
  } from "$lib/components/ui/sheet";
  import { Textarea } from "$lib/components/ui/textarea";
  import {
    customerFormSchema,
    PAYMENT_TERMS_OPTIONS,
    type CustomerFormData,
  } from "$lib/schemas/customer";
  import type { CustomerResponse } from "$lib/types/kikan/CustomerResponse";
  import type { ErrorBody } from "$lib/types/kikan/ErrorBody";
  import { formDirty } from "$lib/actions/form-dirty";
  import { toastApiError } from "$lib/utils/error-toast";
  import { buildUpdatePayload } from "$lib/utils/update-payload";
  import Loader2 from "@lucide/svelte/icons/loader-circle";

  interface Props {
    open: boolean;
    customer?: CustomerResponse;
    onClose: () => void;
  }

  let { open = $bindable(), customer, onClose }: Props = $props();

  let isEdit = $derived(!!customer);
  let submitting = $state(false);
  let fieldErrors = $state<Record<string, string[]>>({});

  const emptyForm: CustomerFormData = {
    display_name: "",
    company_name: "",
    email: "",
    phone: "",
    address_line1: "",
    address_line2: "",
    city: "",
    state: "",
    postal_code: "",
    country: "",
    notes: "",
    payment_terms: "",
    tax_exempt: false,
    credit_limit_cents: undefined,
  };

  let formData = $state<CustomerFormData>({ ...emptyForm });

  let selectedTermsLabel = $derived(
    PAYMENT_TERMS_OPTIONS.find((o) => o.value === formData.payment_terms)
      ?.label ?? "Select terms",
  );

  function customerToForm(c: CustomerResponse): CustomerFormData {
    return {
      display_name: c.display_name,
      company_name: c.company_name ?? "",
      email: c.email ?? "",
      phone: c.phone ?? "",
      address_line1: c.address_line1 ?? "",
      address_line2: c.address_line2 ?? "",
      city: c.city ?? "",
      state: c.state ?? "",
      postal_code: c.postal_code ?? "",
      country: c.country ?? "",
      notes: c.notes ?? "",
      payment_terms: c.payment_terms ?? "",
      tax_exempt: c.tax_exempt,
      credit_limit_cents: c.credit_limit_cents ?? undefined,
    };
  }

  $effect(() => {
    if (!open) return;
    formData = customer ? customerToForm(customer) : { ...emptyForm };
    fieldErrors = {};
  });

  function applyApiErrors(error: ErrorBody) {
    if (error.details) {
      fieldErrors = Object.fromEntries(
        Object.entries(error.details).filter(
          (entry): entry is [string, string[]] =>
            entry[1] !== null && entry[1] !== undefined,
        ),
      );
    } else {
      toastApiError(error, "Failed to save customer. Please try again.");
    }
  }

  async function handleSubmit() {
    const parsed = customerFormSchema.safeParse(formData);
    if (!parsed.success) {
      const errors: Record<string, string[]> = {};
      for (const issue of parsed.error.issues) {
        const key = String(issue.path[0]);
        if (!errors[key]) errors[key] = [];
        errors[key].push(issue.message);
      }
      fieldErrors = errors;
      return;
    }

    submitting = true;
    fieldErrors = {};

    try {
      if (isEdit && customer) {
        const payload = buildUpdatePayload(parsed.data, customer);
        if (Object.keys(payload).length === 0) {
          toast.info("No changes to save");
          open = false;
          onClose();
          return;
        }
        const result = await updateCustomer(customer.id, payload);
        if (result.ok) {
          toast.success(`"${parsed.data.display_name}" updated`);
          open = false;
          onClose();
          await invalidate("app:customers");
        } else {
          applyApiErrors(result.error);
        }
      } else {
        // Strip empty strings to undefined so backend applies defaults
        const createData: Record<string, unknown> = {};
        for (const [key, value] of Object.entries(parsed.data)) {
          createData[key] = value === "" ? undefined : value;
        }
        const result = await createCustomer(createData);
        if (result.ok) {
          toast.success(`"${parsed.data.display_name}" created`);
          open = false;
          onClose();
          await invalidate("app:customers");
        } else {
          applyApiErrors(result.error);
        }
      }
    } finally {
      submitting = false;
    }
  }

  function errorFor(field: string): string | undefined {
    return fieldErrors[field]?.[0];
  }
</script>

<Sheet bind:open>
  <SheetContent side="right" class="overflow-y-auto sm:max-w-lg">
    <SheetHeader>
      <SheetTitle>{isEdit ? "Edit Customer" : "Add Customer"}</SheetTitle>
      <SheetDescription>
        {isEdit
          ? "Update the customer's information."
          : "Enter the new customer's details."}
      </SheetDescription>
    </SheetHeader>

    <form
      class="space-y-4 py-4"
      use:formDirty
      onsubmit={(e) => {
        e.preventDefault();
        handleSubmit();
      }}
    >
      <div class="space-y-1">
        <Label for="display_name">Display Name *</Label>
        <Input
          id="display_name"
          bind:value={formData.display_name}
          disabled={submitting}
          class={errorFor("display_name") ? "border-destructive" : ""}
        />
        {#if errorFor("display_name")}
          <p class="text-sm text-destructive">{errorFor("display_name")}</p>
        {/if}
      </div>

      <div class="space-y-1">
        <Label for="company_name">Company Name</Label>
        <Input
          id="company_name"
          bind:value={formData.company_name}
          disabled={submitting}
        />
      </div>

      <div class="space-y-1">
        <Label for="email">Email</Label>
        <Input
          id="email"
          type="email"
          bind:value={formData.email}
          disabled={submitting}
          class={errorFor("email") ? "border-destructive" : ""}
        />
        {#if errorFor("email")}
          <p class="text-sm text-destructive">{errorFor("email")}</p>
        {/if}
      </div>

      <div class="space-y-1">
        <Label for="phone">Phone</Label>
        <Input
          id="phone"
          type="tel"
          bind:value={formData.phone}
          disabled={submitting}
          class={errorFor("phone") ? "border-destructive" : ""}
        />
        {#if errorFor("phone")}
          <p class="text-sm text-destructive">{errorFor("phone")}</p>
        {/if}
      </div>

      <div class="space-y-2">
        <Label>Address</Label>
        <Input
          placeholder="Street address"
          bind:value={formData.address_line1}
          disabled={submitting}
          class={errorFor("address_line1") ? "border-destructive" : ""}
        />
        {#if errorFor("address_line1")}
          <p class="text-sm text-destructive">{errorFor("address_line1")}</p>
        {/if}
        <Input
          placeholder="Apt, suite, etc."
          bind:value={formData.address_line2}
          disabled={submitting}
          class={errorFor("address_line2") ? "border-destructive" : ""}
        />
        {#if errorFor("address_line2")}
          <p class="text-sm text-destructive">{errorFor("address_line2")}</p>
        {/if}
        <div class="grid grid-cols-2 gap-2">
          <Input
            placeholder="City"
            bind:value={formData.city}
            disabled={submitting}
          />
          <Input
            placeholder="State"
            bind:value={formData.state}
            disabled={submitting}
          />
        </div>
        <div class="grid grid-cols-2 gap-2">
          <Input
            placeholder="Postal code"
            bind:value={formData.postal_code}
            disabled={submitting}
          />
          <Input
            placeholder="Country"
            bind:value={formData.country}
            disabled={submitting}
          />
        </div>
      </div>

      <div class="space-y-1">
        <Label for="notes">Notes</Label>
        <Textarea
          id="notes"
          bind:value={formData.notes}
          disabled={submitting}
        />
      </div>

      <div class="space-y-1">
        <Label for="payment_terms">Payment Terms</Label>
        <Select
          type="single"
          value={formData.payment_terms || undefined}
          onValueChange={(v) => {
            formData.payment_terms = v ?? "";
          }}
          disabled={submitting}
        >
          <SelectTrigger id="payment_terms">
            <span data-slot="select-value">{selectedTermsLabel}</span>
          </SelectTrigger>
          <SelectContent>
            {#each PAYMENT_TERMS_OPTIONS as opt (opt.value)}
              <SelectItem value={opt.value}>{opt.label}</SelectItem>
            {/each}
          </SelectContent>
        </Select>
      </div>
      <div class="flex items-center gap-2">
        <Checkbox
          id="tax_exempt"
          checked={formData.tax_exempt}
          onCheckedChange={(checked) => {
            formData.tax_exempt = checked === true;
          }}
          disabled={submitting}
        />
        <Label for="tax_exempt">Tax Exempt</Label>
      </div>

      <div class="space-y-1">
        <Label for="credit_limit">Credit Limit (cents)</Label>
        <Input
          id="credit_limit"
          type="number"
          min="0"
          value={formData.credit_limit_cents ?? ""}
          oninput={(e) => {
            const val = e.currentTarget.value;
            formData.credit_limit_cents = val === "" ? undefined : Number(val);
          }}
          disabled={submitting}
          class={errorFor("credit_limit_cents") ? "border-destructive" : ""}
        />
        {#if errorFor("credit_limit_cents")}
          <p class="text-sm text-destructive">
            {errorFor("credit_limit_cents")}
          </p>
        {/if}
      </div>

      <SheetFooter>
        <Button type="submit" disabled={submitting}>
          {#if submitting}
            <Loader2 class="mr-2 h-4 w-4 animate-spin" />
          {/if}
          {isEdit ? "Save Changes" : "Create"}
        </Button>
      </SheetFooter>
    </form>
  </SheetContent>
</Sheet>
