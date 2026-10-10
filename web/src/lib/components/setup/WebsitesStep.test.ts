// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import { EventStore } from '#lib/events.svelte.ts';
import type { SetupStep } from '#lib/setup/steps.ts';
import FakeStep from './FakeStep.fixture.svelte';
import SetupWizard from './SetupWizard.svelte';
import WebsitesStep from './WebsitesStep.svelte';

const STEPS: SetupStep[] = [
	{ id: 'websites', title: 'Websites', component: WebsitesStep },
	{ id: 'after', title: 'After', component: FakeStep }
];

/** A fresh install's server, which selects no website. */
async function open() {
	const api = createApi({
		baseUrl: 'http://fmd2r.test',
		fetch: createMockBackend({ setUp: false }).fetch
	});
	const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
	render(SetupWizard, { api, store, steps: STEPS, onfinish: vi.fn() });
	await screen.findByRole('checkbox', { name: 'MangaDex' });
	return api;
}

const next = () => screen.getByRole('button', { name: 'Next' }) as HTMLButtonElement;

describe('the websites setup step', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('needs a website before Next, and saves the selected ones', async () => {
		const api = await open();
		expect(next().disabled).toBe(true);
		expect(screen.getByText(/Select at least one website/)).toBeTruthy();

		await fireEvent.click(screen.getByRole('checkbox', { name: 'MangaDex' }));
		await fireEvent.click(screen.getByRole('checkbox', { name: 'Webtoons' }));
		expect(next().disabled).toBe(false);
		expect(screen.queryByText(/Select at least one website/)).toBeNull();

		await fireEvent.click(next());
		await screen.findByRole('heading', { level: 2, name: 'After' });
		expect((await api.getSettings()).general.selected_websites).toEqual(['mangadex', 'webtoons']);
	});
});
