import tailwindcss from "@tailwindcss/vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { svelteTesting } from "@testing-library/svelte/vite";
import { configDefaults, defineConfig } from "vitest/config";

// The api_version the admin SPA was built against. Compared at boot
// against `GET /api/kikan-version` to detect engine/UI drift. Must stay
// in lockstep with `kikan_types::API_VERSION` in
// `crates/kikan-types/src/lib.rs` — a vitest drift guard pins that.
const KIKAN_ADMIN_UI_BUILT_FOR = "1.0.0";

export default defineConfig({
  plugins: [tailwindcss(), sveltekit(), svelteTesting()],
  define: {
    __KIKAN_ADMIN_UI_BUILT_FOR__: JSON.stringify(KIKAN_ADMIN_UI_BUILT_FOR),
  },
  test: {
    passWithNoTests: true,
    setupFiles: ["vitest-setup.ts"],
    exclude: [
      ...configDefaults.exclude,
      "**/.claude/**",
      ".features-gen/**",
      "tests/demo-captures/**",
      "tests/smoke/**",
    ],
    coverage: {
      provider: "v8",
      reporter: ["json", "text"],
      include: ["src/**/*.ts", "src/**/*.svelte"],
      exclude: ["src/**/index.ts"],
    },
  },
});
