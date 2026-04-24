// See https://svelte.dev/docs/kit/types#app.d.ts
declare global {
  namespace App {
    // interface Error {}
    // interface Locals {}
    // interface PageData {}
    interface PageState {
      fromWelcome?: boolean;
    }
    // interface Platform {}
  }

  /**
   * `kikan_types::API_VERSION` at the time this SPA was built. Injected by
   * Vite `define` in `vite.config.ts`. Compared at boot against
   * `GET /api/kikan-version` to detect engine/UI drift.
   */
  const __KIKAN_ADMIN_UI_BUILT_FOR__: string;
}

export {};
