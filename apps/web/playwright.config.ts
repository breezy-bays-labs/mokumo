import { defineConfig } from '@playwright/test';
import { defineBddConfig } from 'playwright-bdd';

const storybookTestDir = defineBddConfig({
	outputDir: '.features-gen/storybook',
	features: [
		'tests/features/**/*.feature',
		'!tests/features/settings/**/*.feature',
		'!tests/features/customers/**/*.feature',
	],
	steps: ['tests/steps/*.ts', '!tests/steps/settings-lan.steps.ts', 'tests/support/storybook.fixture.ts', 'tests/support/storybook.helpers.ts'],
	tags: 'not @wip',
});

const appTestDir = defineBddConfig({
	outputDir: '.features-gen/app',
	features: [
		'tests/features/settings/**/*.feature',
		'tests/features/customers/**/*.feature',
	],
	steps: [
		'tests/steps/settings-lan.steps.ts',
		'tests/steps/customer-*.steps.ts',
		'tests/support/app.fixture.ts',
	],
	importTestFrom: 'tests/support/app.fixture.ts',
	tags: 'not @wip',
	disableWarnings: { importTestFrom: true },
});

export default defineConfig({
	workers: 2,
	projects: [
		{
			name: 'storybook',
			testDir: storybookTestDir,
			use: { browserName: 'chromium' },
		},
		{
			name: 'app',
			testDir: appTestDir,
			use: { browserName: 'chromium' },
		},
	],
	reporter: 'html',
	timeout: 30_000,
});
