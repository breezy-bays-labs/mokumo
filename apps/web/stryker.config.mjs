import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);

/** @type {import('@stryker-mutator/api/core').PartialStrykerOptions} */
export default {
	testRunner: 'vitest',
	plugins: [
		require.resolve('@stryker-mutator/vitest-runner'),
	],
	mutate: ['src/lib/**/*.ts', '!src/lib/**/*.test.ts', '!src/lib/**/index.ts', '!src/lib/types/**', '!src/lib/components/**', '!src/lib/mocks/**'],
	thresholds: { high: 80, low: 60, break: 50 },
};
