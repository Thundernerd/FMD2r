import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defaultClientConditions } from 'vite';
import { defineConfig } from 'vitest/config';

// A mock-mode build (used by the Playwright smoke tests) must never land in `build/`, which T21 embeds.
const out = process.env.VITE_API_MOCK === 'true' ? '.svelte-kit/build-mock' : 'build';

export default defineConfig({
	plugins: [
		sveltekit({
			// SPA: every route falls back to index.html; fmd-server (T21) embeds `build/`.
			adapter: adapter({ pages: out, assets: out, fallback: 'index.html' })
		})
	],
	server: {
		// When running against a real backend (`VITE_API_MOCK` unset), forward API calls to `fmd2r serve`.
		proxy: { '/api': process.env.FMD2R_URL ?? 'http://127.0.0.1:8080' }
	},
	// Component tests mount Svelte in jsdom, which needs Svelte's browser build, not its server one.
	resolve: process.env.VITEST ? { conditions: [...defaultClientConditions] } : undefined,
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node'
	}
});
