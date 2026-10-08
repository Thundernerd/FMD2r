import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
	testDir: 'e2e',
	webServer: {
		// Smoke tests run against a production build in mock mode, so no backend is needed.
		command: 'npm run build && npm run preview -- --port 4173 --strictPort',
		env: { VITE_API_MOCK: 'true' },
		port: 4173,
		reuseExistingServer: !process.env.CI
	},
	use: { baseURL: 'http://localhost:4173' },
	projects: [
		{ name: 'desktop', use: { ...devices['Desktop Chrome'] } },
		{ name: 'phone', use: { ...devices['Desktop Chrome'], viewport: { width: 375, height: 740 } } }
	]
});
