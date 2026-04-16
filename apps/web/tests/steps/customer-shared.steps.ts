import { expect, type APIRequestContext } from "@playwright/test";
import { createCustomer, type CreateCustomerBody } from "../../src/lib/fixtures/customer";
import type { CustomerResponse } from "../../src/lib/types/kikan/CustomerResponse";
import { Given, When, Then } from "../support/app.fixture";

// --- Seeding Helpers ---

export async function seedCustomer(
  apiContext: APIRequestContext,
  overrides: Partial<CreateCustomerBody> = {},
): Promise<CustomerResponse> {
  const body = createCustomer(overrides);
  const response = await apiContext.post("/api/customers", { data: body });
  if (!response.ok()) {
    const errorBody = await response.text();
    throw new Error(`seedCustomer failed (${response.status()}): ${errorBody}`);
  }
  return response.json();
}

export async function seedCustomers(
  apiContext: APIRequestContext,
  count: number,
): Promise<CustomerResponse[]> {
  const results: CustomerResponse[] = [];
  for (let i = 0; i < count; i++) {
    results.push(await seedCustomer(apiContext, { display_name: `Customer ${i + 1}` }));
  }
  return results;
}

export async function archiveCustomer(
  apiContext: APIRequestContext,
  id: number | string,
): Promise<void> {
  const response = await apiContext.delete(`/api/customers/${id}`);
  if (!response.ok()) {
    const errorBody = await response.text();
    throw new Error(`archiveCustomer failed (${response.status()}): ${errorBody}`);
  }
}

// --- Shared Given Steps ---

Given("no customers exist in the system", async ({ freshBackend }) => {
  void freshBackend;
});

Given("customers exist in the system", async ({ freshBackend, apiContext, customerContext }) => {
  void freshBackend;
  const customers = await seedCustomers(apiContext, 5);
  customerContext.customers = customers;
  customerContext.lastCustomer = customers[customers.length - 1];
});

Given(
  "{int} customers exist in the system",
  async ({ freshBackend, apiContext, customerContext }, count: number) => {
    void freshBackend;
    const customers = await seedCustomers(apiContext, count);
    customerContext.customers = customers;
    customerContext.lastCustomer = customers[customers.length - 1];
  },
);

Given(
  "a customer {string} exists",
  async ({ freshBackend, apiContext, customerContext }, name: string) => {
    void freshBackend;
    const customer = await seedCustomer(apiContext, { display_name: name });
    customerContext.customers = [customer];
    customerContext.lastCustomer = customer;
  },
);

Given(
  "customers {string} and {string} exist",
  async ({ freshBackend, apiContext, customerContext }, name1: string, name2: string) => {
    void freshBackend;
    const c1 = await seedCustomer(apiContext, { display_name: name1 });
    const c2 = await seedCustomer(apiContext, { display_name: name2 });
    customerContext.customers = [c1, c2];
    customerContext.lastCustomer = c2;
  },
);

Given(
  "an archived customer {string} exists",
  async ({ freshBackend, apiContext, customerContext }, name: string) => {
    void freshBackend;
    const customer = await seedCustomer(apiContext, { display_name: name });
    await archiveCustomer(apiContext, customer.id);
    customerContext.customers = [customer];
    customerContext.lastCustomer = customer;
  },
);

// --- Shared When Steps ---

When("I navigate to the Customers page", async ({ axumUrl, page }) => {
  await page.goto(`${axumUrl}/customers`);
  await expect(
    page.getByRole("heading", { name: "Customers" }).or(page.getByText("No customers yet")),
  ).toBeVisible({ timeout: 10_000 });
});

// --- Shared Then Steps ---

Then("I see a table of customers", async ({ page }) => {
  await expect(page.locator("table")).toBeVisible();
});

Then("{string} appears in the table", async ({ page }, name: string) => {
  await expect(page.locator("table").getByText(name)).toBeVisible();
});

Then("{string} does not appear in the table", async ({ page }, name: string) => {
  await expect(page.locator("table").getByText(name)).toHaveCount(0);
});
