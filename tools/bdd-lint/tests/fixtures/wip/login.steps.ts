import { Given, When, Then } from "./support.ts";

// Matches current login flow
Given("a user with email {string}", async () => {});
When("the user enters their password", async () => {});
Then("the user is logged in", async () => {});

// Only matches @wip scenario — should be detected as orphan
Given("a user with SSO enabled", async () => {});
When("the user authenticates via SSO", async () => {});
Then("the user is redirected to the dashboard", async () => {});
