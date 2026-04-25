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
      ".svelte-kit/**",
      "build/**",
      "tests/features/**",
      "tests/screens/**",
      ".features-gen/**",
    ],
    coverage: {
      provider: "v8",
      reporter: ["json", "text"],
      include: ["src/**/*.ts", "src/**/*.svelte"],
      exclude: ["src/**/index.ts"],
    },
  },
});
