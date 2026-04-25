import { defineConfig } from "@playwright/test";
import { defineBddConfig } from "playwright-bdd";

const bddTestDir = defineBddConfig({
  outputDir: ".features-gen/pr2a",
  features: ["tests/features/*.feature"],
  steps: ["tests/steps/*.steps.ts", "tests/support/app.fixture.ts"],
  importTestFrom: "tests/support/app.fixture.ts",
  tags: "@pr2a",
  disableWarnings: { importTestFrom: true },
});

export default defineConfig({
  workers: 2,
  projects: [
    {
      name: "pr2a",
      testDir: bddTestDir,
      use: {
        browserName: "chromium",
        baseURL: "http://localhost:5173",
        // The setup wizard's "Copy shop URL" scenario reads navigator.clipboard
        // to verify the URL was written. Chromium gates clipboard access behind
        // a permission; grant it for the dev origin so the BDD assertion runs.
        permissions: ["clipboard-read", "clipboard-write"],
      },
    },
  ],
  webServer: {
    command: "pnpm exec vite dev --port 5173 --strictPort",
    url: "http://localhost:5173/admin/",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    stdout: "pipe",
    stderr: "pipe",
  },
  reporter: process.env.CI ? "list" : "html",
  timeout: 30_000,
});
