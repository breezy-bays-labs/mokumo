import { expect } from "@playwright/test";
import { Given, Then, When } from "../support/app.fixture";
import { archiveCustomer, seedCustomer } from "./customer-shared.steps";

Then("a confirmation dialog appears", async ({ page }) => {
  await expect(page.getByRole("alertdialog").or(page.getByRole("dialog"))).toBeVisible();
});

When("I confirm the archive", async ({ page }) => {
  const dialog = page.getByRole("alertdialog").or(page.getByRole("dialog"));
  await dialog.getByRole("button", { name: "Archive" }).click();
});

Then("I am redirected to the Customers page", async ({ page }) => {
  await expect(page).toHaveURL(/\/customers$/);
});

Given(
  "{string} has been archived",
  async ({ freshBackend, apiContext, customerContext }, name: string) => {
    void freshBackend;
    const customer = await seedCustomer(apiContext, { display_name: name });
    await archiveCustomer(apiContext, customer.id);
    customerContext.customers = [customer];
    customerContext.lastCustomer = customer;
  },
);

Given(
  "I am on the detail page for archived customer {string}",
  async ({ axumUrl, page, customerContext }, name: string) => {
    const customer = customerContext.customers.find((c) => c.display_name === name);
    expect(customer).toBeTruthy();
    await page.goto(`${axumUrl}/customers/${customer!.id}`);
    await expect(page.getByRole("heading", { name })).toBeVisible({ timeout: 10_000 });
    await expect(page.getByText("Archived")).toBeVisible();
  },
);

When("I confirm the restore", async ({ page }) => {
  const dialog = page.getByRole("alertdialog").or(page.getByRole("dialog"));
  await dialog.getByRole("button", { name: "Restore" }).click();
});

Then("a success toast appears with text {string} restored", async ({ page }, name: string) => {
  await expect(page.getByText(`"${name}" restored`)).toBeVisible({ timeout: 5_000 });
});

Then("the Archived badge is no longer visible", async ({ page }) => {
  await expect(page.getByText("Archived")).toHaveCount(0, { timeout: 5_000 });
});
