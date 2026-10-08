import { defineConfig, devices } from '@playwright/test';

// Smoke tests against a real `fmd2r serve` (debug build, serving `build/`) with a fixture module
// and a local image site; see e2e-real/serve.mjs. Needs the Rust toolchain, so CI does not run it.
export default defineConfig({
	testDir: 'e2e-real',
	timeout: 60_000,
	webServer: {
		command: 'npm run build && node e2e-real/serve.mjs',
		// The fixture site answers /ready once the server is up and configured.
		url: 'http://127.0.0.1:4181/ready',
		timeout: 600_000,
		reuseExistingServer: false
	},
	use: { baseURL: 'http://127.0.0.1:4180' },
	projects: [{ name: 'desktop', use: { ...devices['Desktop Chrome'] } }]
});
