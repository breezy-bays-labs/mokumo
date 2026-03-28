import { z } from "zod";

export const PAYMENT_TERMS_OPTIONS = [
  { value: "due_on_receipt", label: "Due on Receipt" },
  { value: "net_15", label: "Net 15" },
  { value: "net_30", label: "Net 30" },
  { value: "net_60", label: "Net 60" },
] as const;

// PARITY: phone regex must match PHONE_RE in crates/core/src/customer/service.rs
const optionalPhone = z
  .string()
  .optional()
  .refine((val) => !val || (/^[+]?[\d\s\-().]+$/.test(val) && /\d/.test(val)), {
    message: "Invalid phone number format",
  });

// PARITY: address check must match validate_address in crates/core/src/customer/service.rs
const optionalAddress = z
  .string()
  .optional()
  .refine((val) => !val || /[a-zA-Z0-9]/.test(val), {
    message: "Address contains invalid characters",
  });

export const customerFormSchema = z.object({
  display_name: z.string().trim().min(1, "Display name is required"),
  company_name: z.string().optional(),
  email: z.string().email("Invalid email address").optional().or(z.literal("")),
  phone: optionalPhone,
  address_line1: optionalAddress,
  address_line2: optionalAddress,
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
