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
	if (typeof localStorage !== 'undefined') {
		localStorage.clear();
	}
	if (typeof sessionStorage !== 'undefined') {
		sessionStorage.clear();
	}
});
