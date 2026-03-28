import { defineConfig } from '@playwright/test';
import { defineBddConfig } from 'playwright-bdd';

const storybookTestDir = defineBddConfig({
	outputDir: '.features-gen/storybook',
	features: [
		'tests/features/**/*.feature',
		'!tests/features/settings/**/*.feature',
		'!tests/features/customers/**/*.feature',
		'!tests/features/dashboard.feature',
		'!tests/features/setup-wizard.feature',
		'!tests/features/help-popover/**/*.feature',
	],
	steps: [
		'tests/steps/*.ts',
		'!tests/steps/settings-lan.steps.ts',
		'!tests/steps/shared-lan.steps.ts',
		'!tests/steps/dashboard.steps.ts',
		'!tests/steps/setup-wizard.steps.ts',
		'!tests/steps/help-popover.steps.ts',
		'!tests/steps/customer-*.steps.ts',
		'tests/support/storybook.fixture.ts',
		'tests/support/storybook.helpers.ts',
	],
	tags: 'not @wip and not @future',
});

const appTestDir = defineBddConfig({
	outputDir: '.features-gen/app',
	features: [
		'tests/features/settings/**/*.feature',
		'tests/features/customers/**/*.feature',
		'tests/features/help-popover/**/*.feature',
	],
	steps: [
		'tests/steps/settings-lan.steps.ts',
		'tests/steps/shared-lan.steps.ts',
		'tests/steps/customer-*.steps.ts',
		'tests/steps/help-popover.steps.ts',
		'tests/support/app.fixture.ts',
	],
	importTestFrom: 'tests/support/app.fixture.ts',
	tags: 'not @wip and not @future',
	disableWarnings: { importTestFrom: true },
});

const onboardingTestDir = defineBddConfig({
	outputDir: '.features-gen/onboarding',
	features: [
		'tests/features/dashboard.feature',
		'tests/features/setup-wizard.feature',
	],
	steps: [
		'tests/steps/dashboard.steps.ts',
		'tests/steps/setup-wizard.steps.ts',
		'tests/steps/shared-lan.steps.ts',
		'tests/support/app.fixture.ts',
	],
	importTestFrom: 'tests/support/app.fixture.ts',
	tags: 'not @wip and not @future',
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
		{
			name: 'onboarding',
			testDir: onboardingTestDir,
			use: { browserName: 'chromium' },
		},
		{
			name: 'demo-captures',
			testDir: 'tests/demo-captures',
			use: {
				browserName: 'chromium',
				screenshot: 'only-on-failure',
				trace: 'retain-on-failure',
			},
		},
	],
	reporter: 'html',
	timeout: 30_000,
});
