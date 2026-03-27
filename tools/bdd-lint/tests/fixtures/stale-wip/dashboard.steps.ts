import { Given, When, Then } from "./support.ts";

// Shared steps — match all scenarios
Given("the user is logged in", async () => {});
When("the user navigates to the dashboard", async () => {});

// Steps for "User views dashboard stats" (@wip) — ALL matched → stale
Then("the dashboard shows total orders", async () => {});

// Steps for "User sees welcome message" (non-wip)
Then("a welcome message is displayed", async () => {});

// Steps for "User exports dashboard report" (@wip) — export step has NO def → not stale
// When("the user clicks export to PDF") is intentionally missing
// Then("a PDF report is downloaded") is intentionally missing
