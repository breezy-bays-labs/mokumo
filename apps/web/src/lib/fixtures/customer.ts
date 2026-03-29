import { faker } from "@faker-js/faker";

/**
 * Body for POST /api/customers — matches Rust CreateCustomer struct.
 * Only display_name is required; all other fields are optional.
 */
export type CreateCustomerBody = {
  display_name: string;
  company_name?: string | null;
  email?: string | null;
  phone?: string | null;
  address_line1?: string | null;
  address_line2?: string | null;
  city?: string | null;
  state?: string | null;
  postal_code?: string | null;
  country?: string | null;
  notes?: string | null;
  portal_enabled?: boolean | null;
  tax_exempt?: boolean | null;
  payment_terms?: string | null;
  credit_limit_cents?: number | null;
  lead_source?: string | null;
  tags?: string | null;
};

type CustomerTemplate = "full" | "standard" | "minimal";

const LEAD_SOURCES = ["referral", "website", "trade-show", "cold-call", "social-media", "repeat"];
const PAYMENT_TERMS = ["net_15", "net_30", "net_60", "due_on_receipt", "cod"];
const TAG_OPTIONS = [
  "screen-print",
  "embroidery",
  "dtf",
  "dtg",
  "vinyl",
  "wholesale",
  "retail",
  "rush-jobs",
  "recurring",
  "vip",
];

function pickWeightedTemplate(): CustomerTemplate {
  const roll = faker.number.float({ min: 0, max: 1 });
  if (roll < 0.2) return "minimal";
  if (roll < 0.7) return "standard";
  return "full";
}

function randomTags(): string {
  const count = faker.number.int({ min: 1, max: 3 });
  return faker.helpers.arrayElements(TAG_OPTIONS, count).join(",");
}

function createFullCustomer(): CreateCustomerBody {
  return {
    display_name: faker.person.fullName(),
    company_name: faker.company.name(),
    email: faker.internet.email(),
    phone: faker.phone.number({ style: "national" }),
    address_line1: faker.location.streetAddress(),
    address_line2: faker.helpers.maybe(() => faker.location.secondaryAddress()) ?? null,
    city: faker.location.city(),
    state: faker.location.state({ abbreviated: true }),
    postal_code: faker.location.zipCode(),
    country: "US",
    notes: faker.lorem.sentence(),
    portal_enabled: faker.datatype.boolean(),
    tax_exempt: faker.datatype.boolean({ probability: 0.15 }),
    payment_terms: faker.helpers.arrayElement(PAYMENT_TERMS),
    credit_limit_cents: faker.number.int({ min: 50000, max: 500000 }),
    lead_source: faker.helpers.arrayElement(LEAD_SOURCES),
    tags: randomTags(),
  };
}

function createStandardCustomer(): CreateCustomerBody {
  return {
    display_name: faker.person.fullName(),
    company_name: faker.company.name(),
    email: faker.internet.email(),
    phone: faker.phone.number({ style: "national" }),
  };
}

function createMinimalCustomer(): CreateCustomerBody {
  return {
    display_name: faker.person.fullName(),
  };
}

const templateFactories: Record<CustomerTemplate, () => CreateCustomerBody> = {
  full: createFullCustomer,
  standard: createStandardCustomer,
  minimal: createMinimalCustomer,
};

/**
 * Create a single customer with random weighted template selection.
 * Override any field with the overrides parameter.
 */
export function createCustomer(overrides: Partial<CreateCustomerBody> = {}): CreateCustomerBody {
  const template = pickWeightedTemplate();
  return { ...templateFactories[template](), ...overrides };
}

/** Hand-crafted hero customers for demo databases. */
const HERO_CUSTOMERS: CreateCustomerBody[] = [
  {
    display_name: "Gary Thompson",
    company_name: "4Ink Custom Prints",
    email: "gary@4inkcustomprints.com",
    phone: "(512) 555-0142",
    address_line1: "2847 Commerce Dr",
    address_line2: "Suite 110",
    city: "Austin",
    state: "TX",
    postal_code: "78745",
    country: "US",
    notes: "Long-time screen print shop. Specializes in athletic wear and team jerseys.",
    portal_enabled: true,
    tax_exempt: false,
    payment_terms: "net_30",
    credit_limit_cents: 250000,
    lead_source: "referral",
    tags: "screen-print,wholesale,recurring,vip",
  },
  {
    display_name: "Sarah Chen",
    company_name: "Threadworks Studio",
    email: "sarah@threadworks.co",
    phone: "(503) 555-0198",
    address_line1: "1420 NE Alberta St",
    city: "Portland",
    state: "OR",
    postal_code: "97211",
    country: "US",
    notes: "Boutique embroidery and DTF studio. High-quality small runs, corporate clients.",
    portal_enabled: true,
    tax_exempt: false,
    payment_terms: "net_15",
    credit_limit_cents: 150000,
    lead_source: "trade-show",
    tags: "embroidery,dtf,retail,vip",
  },
];

/**
 * Create a hero customer by index (0 = Gary Thompson, 1 = Sarah Chen).
 */
export function createHeroCustomer(index: 0 | 1): CreateCustomerBody {
  return { ...HERO_CUSTOMERS[index] };
}

/**
 * Generate an array of customers for seeding: 2 heroes first, then random weighted customers.
 * Pass a seed for deterministic output.
 */
export function seedCustomers(count: number, seed?: number): CreateCustomerBody[] {
  if (count < 2) throw new Error("seedCustomers requires count >= 2 (for hero customers)");

  if (seed !== undefined) {
    faker.seed(seed);
  }

  const customers: CreateCustomerBody[] = [createHeroCustomer(0), createHeroCustomer(1)];

  for (let i = 2; i < count; i++) {
    customers.push(createCustomer());
  }

  return customers;
}
