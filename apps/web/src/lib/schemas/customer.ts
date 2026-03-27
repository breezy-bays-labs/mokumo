import { z } from "zod";

export const PAYMENT_TERMS_OPTIONS = [
  { value: "due_on_receipt", label: "Due on Receipt" },
  { value: "net_15", label: "Net 15" },
  { value: "net_30", label: "Net 30" },
  { value: "net_60", label: "Net 60" },
] as const;

export const customerFormSchema = z.object({
  display_name: z.string().trim().min(1, "Display name is required"),
  company_name: z.string().optional(),
  email: z.string().email("Invalid email address").optional().or(z.literal("")),
  phone: z.string().optional(),
  address_line1: z.string().optional(),
  address_line2: z.string().optional(),
  city: z.string().optional(),
  state: z.string().optional(),
  postal_code: z.string().optional(),
  country: z.string().optional(),
  notes: z.string().optional(),
  payment_terms: z.string().optional(),
  tax_exempt: z.boolean().optional(),
  credit_limit_cents: z.coerce.number().int().min(0, "Must be 0 or greater").optional(),
});

export type CustomerFormData = z.infer<typeof customerFormSchema>;

/**
 * Schema for customer list URL parameters.
 * Used with useSearchParams (component-side) for two-way URL binding.
 * All fields have defaults so useSearchParams can initialize from empty URL.
 */
export const customerListParamsSchema = z.object({
  search: z.string().default(""),
  page: z.coerce.number().int().min(1).default(1),
  per_page: z.coerce.number().int().min(1).max(100).default(25),
  include_deleted: z.boolean().default(false),
});

export type CustomerListParams = z.infer<typeof customerListParamsSchema>;
