import { defineConfig, devices } from '@playwright/test';

// End-to-end tests against a real `fmd2r serve` (debug build, serving `build/`) with a fixture
// module and a local image site; see e2e-real/serve.mjs. CI runs them in ci.yml's `e2e` job.
export default defineConfig({
	testDir: 'e2e-real',
	// The projects share one server, and a test restarts it.
	workers: 1,
	timeout: 120_000,
	forbidOnly: !!process.env.CI,
	reporter: process.env.CI ? [['list'], ['github']] : 'list',
	webServer: {
		command: 'npm run build && node e2e-real/serve.mjs',
		// The fixture site answers /ready once the server is up and configured.
		url: 'http://127.0.0.1:4181/ready',
		timeout: 600_000,
		reuseExistingServer: false
	},
	use: { baseURL: 'http://127.0.0.1:4180', trace: 'retain-on-failure' },
	projects: [
		{ name: 'desktop', use: { ...devices['Desktop Chrome'] } },
		{ name: 'phone', use: { ...devices['Desktop Chrome'], viewport: { width: 375, height: 740 } } }
	]
});
