import { test, expect } from "../support/demo.fixture";

test.describe("M0 Demo Screenshots", () => {
  test.describe.configure({ mode: "serial" });
  test.setTimeout(120_000);

  test("smoke: Axum server responds", async ({ demoPage }) => {
    const response = await demoPage.request.get("/api/health");
    expect(response.ok()).toBe(true);
  });
});
