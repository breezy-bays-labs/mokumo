import "@testing-library/jest-dom/vitest";
import { vi, beforeEach } from "vitest";

// Default browser: false — component test files override to true per-file
vi.mock("$app/environment", () => ({ browser: false, dev: false, building: false }));
vi.mock("$app/navigation", () => ({
  goto: vi.fn(),
  invalidate: vi.fn(),
  invalidateAll: vi.fn(),
}));

// Node 25+ ships built-in localStorage/sessionStorage globals that exist but
// lack Storage methods (getItem, setItem, clear, etc.) when the backing file
// flags are not configured. vi.unstubAllGlobals() in test afterEach blocks can
// also restore these broken stubs. Call this helper in beforeEach so every test
// starts with a functional in-memory implementation regardless of Node version.
function ensureStorage(name: "localStorage" | "sessionStorage"): void {
  if (typeof globalThis[name] !== "undefined" && typeof globalThis[name].clear !== "function") {
    const store = new Map<string, string>();
    vi.stubGlobal(name, {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => {
        store.set(k, v);
      },
      removeItem: (k: string) => {
        store.delete(k);
      },
      clear: () => {
        store.clear();
      },
      key: (i: number) => [...store.keys()][i] ?? null,
      get length() {
        return store.size;
      },
    });
  }
}

beforeEach(() => {
  ensureStorage("localStorage");
  ensureStorage("sessionStorage");

  if (typeof localStorage !== "undefined" && typeof localStorage.clear === "function") {
    localStorage.clear();
  }
  if (typeof sessionStorage !== "undefined" && typeof sessionStorage.clear === "function") {
    sessionStorage.clear();
  }
});
