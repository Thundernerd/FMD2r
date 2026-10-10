import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

// A mock-mode build (used by the Playwright smoke tests) must never land in `build/`, which T21 embeds.
const mock = process.env.VITE_API_MOCK === 'true';
const out = mock ? '.svelte-kit/build-mock' : 'build';
const backend = process.env.FMD2R_URL ?? 'http://127.0.0.1:8080';

export default defineConfig({
	plugins: [
		sveltekit({
			// SPA: every route falls back to index.html; fmd-server (T21) embeds `build/`.
			adapter: adapter({ pages: out, assets: out, fallback: 'index.html' })
		}),
		// Component tests (`// @vitest-environment jsdom`) mount Svelte's browser build.
		svelteTesting()
	],
	server: {
		// When running against a real backend (`VITE_API_MOCK` unset), forward API calls and the user's
		// `custom.css` to `fmd2r serve`. `vite preview` proxies too, so mock mode leaves the CSS out.
		proxy: {
			'/api': backend,
			...(mock ? {} : { '/custom.css': backend })
		}
	},
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node'
	}
});
