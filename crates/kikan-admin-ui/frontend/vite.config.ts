import tailwindcss from "@tailwindcss/vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { svelteTesting } from "@testing-library/svelte/vite";
import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [tailwindcss(), sveltekit(), svelteTesting()],
  test: {
    passWithNoTests: true,
    setupFiles: ["vitest-setup.ts"],
    exclude: [
      ...configDefaults.exclude,
      // adr: docs/adr/adr-coverage-exclusions.md — Generated and runner-scope filters
      ".svelte-kit/**",
      "build/**",
      // adr: docs/adr/adr-coverage-exclusions.md
      "tests/features/**",
      "tests/screens/**",
      // adr: docs/adr/adr-coverage-exclusions.md
      ".features-gen/**",
    ],
    coverage: {
      provider: "v8",
      reporter: ["json", "text"],
      include: ["src/**/*.ts", "src/**/*.svelte"],
      // adr: docs/adr/adr-coverage-exclusions.md — Barrel-file re-exports (no logic)
      exclude: ["src/**/index.ts"],
    },
  },
});
