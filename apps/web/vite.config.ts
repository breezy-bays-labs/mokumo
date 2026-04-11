import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { configDefaults, defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit(), svelteTesting()],
	test: {
		passWithNoTests: true,
		setupFiles: ['vitest-setup.ts'],
		exclude: [...configDefaults.exclude, '**/.claude/**', '.features-gen/**', 'tests/demo-captures/**', 'tests/smoke/**'],
		coverage: {
			provider: 'v8',
			reporter: ['json', 'text'],
			include: ['src/**/*.ts', 'src/**/*.svelte'],
			exclude: ['src/**/index.ts']
		}
	}
});
