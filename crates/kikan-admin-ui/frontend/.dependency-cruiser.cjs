/** @type {import('dependency-cruiser').IConfiguration} */
module.exports = {
	forbidden: [
		{
			name: 'no-circular',
			comment: 'No circular dependencies allowed',
			severity: 'error',
			from: {},
			to: {
				circular: true,
			},
		},
		{
			name: 'lib-no-route-imports',
			comment: 'Shared library code must not depend on route-specific modules',
			severity: 'error',
			from: { path: '^src/lib/' },
			to: {
				path: '^src/routes/',
			},
		},
		{
			name: 'types-no-implementation-imports',
			comment: 'Type definitions must not import from implementation modules',
			severity: 'error',
			from: { path: '^src/lib/types/' },
			to: {
				path: '^src/lib/',
				pathNot: '^src/lib/types/',
			},
		},
		{
			name: 'no-test-imports-in-production',
			comment: 'Production code must not import from test files',
			severity: 'error',
			from: { pathNot: '\\.(test|spec)\\.' },
			to: {
				path: '\\.(test|spec)\\.',
			},
		},
	],
	options: {
		doNotFollow: {
			path: 'node_modules',
		},
		tsPreCompilationDeps: true,
		tsConfig: {
			fileName: 'tsconfig.json',
		},
	},
};
