import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["tests/**/*.test.ts"],
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts"],
      // adr: docs/adr/adr-coverage-exclusions.md — Barrel-file re-exports (no logic)
      exclude: ["src/index.ts"],
      thresholds: {
        lines: 90,
      },
    },
  },
});
