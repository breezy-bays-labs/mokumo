import '@testing-library/jest-dom/vitest';
import { vi, beforeEach } from 'vitest';

// Default browser: false — component test files override to true per-file
vi.mock('$app/environment', () => ({ browser: false, dev: false, building: false }));
vi.mock('$app/navigation', () => ({
	goto: vi.fn(),
	invalidate: vi.fn(),
	invalidateAll: vi.fn()
}));

beforeEach(() => {
	// Node 25+ ships a built-in localStorage global that exists but lacks Storage
	// methods (getItem, setItem, clear, etc.) when --localstorage-file is not
	// configured. vi.unstubAllGlobals() in any test's afterEach can also restore
	// this broken stub. Re-detect and replace each time so every test starts with
	// a functional in-memory implementation.
	if (typeof localStorage !== 'undefined' && typeof localStorage.clear !== 'function') {
		const store = new Map<string, string>();
		vi.stubGlobal('localStorage', {
			getItem: (k: string) => store.get(k) ?? null,
			setItem: (k: string, v: string) => { store.set(k, v); },
			removeItem: (k: string) => { store.delete(k); },
			clear: () => { store.clear(); },
			key: (i: number) => [...store.keys()][i] ?? null,
			get length() { return store.size; },
		});
	}

	if (typeof localStorage !== 'undefined' && typeof localStorage.clear === 'function') {
		localStorage.clear();
	}
	if (typeof sessionStorage !== 'undefined' && typeof sessionStorage.clear === 'function') {
		sessionStorage.clear();
	}
});
