import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);

/** @type {import('@stryker-mutator/api/core').PartialStrykerOptions} */
export default {
	testRunner: 'vitest',
	checkers: ['typescript'],
	tsconfigFile: 'tsconfig.json',
	plugins: [
		require.resolve('@stryker-mutator/vitest-runner'),
		require.resolve('@stryker-mutator/typescript-checker'),
	],
	mutate: ['src/lib/**/*.ts', '!src/lib/**/*.test.ts', '!src/lib/**/index.ts', '!src/lib/types/**'],
	thresholds: { high: 80, low: 60, break: 50 },
};
