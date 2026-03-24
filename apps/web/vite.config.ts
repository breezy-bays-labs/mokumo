import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	test: {
		passWithNoTests: true,
		exclude: ['**/node_modules/**', '**/.claude/**'],
		coverage: {
			provider: 'v8',
			reporter: ['json', 'text'],
			include: ['src/**/*.ts', 'src/**/*.svelte'],
			exclude: ['src/**/index.ts']
		}
	}
});
