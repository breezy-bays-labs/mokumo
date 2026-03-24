import { defineConfig } from '@playwright/test';
import { defineBddConfig } from 'playwright-bdd';

const testDir = defineBddConfig({
	features: 'tests/features/*.feature',
	steps: ['tests/steps/*.ts', 'tests/support/*.ts'],
	tags: 'not @wip'
});

export default defineConfig({
	testDir,
	workers: 1,
	use: {
		baseURL: 'http://localhost:6006'
	},
	projects: [
		{
			name: 'storybook',
			use: { browserName: 'chromium' }
		}
	],
	reporter: 'html',
	timeout: 30_000
});
