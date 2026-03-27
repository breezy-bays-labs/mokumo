import { Given, When, Then } from "./support.ts";

Given("a user with an active subscription", async () => {});
When("the user opens the billing page", async () => {});
Then("the invoice is displayed", async () => {});

// Orphan definitions — no scenario references these
Given("the user has a payment method on file", async () => {});
When("the user cancels their subscription", async () => {});
Then("the refund is processed", async () => {});
