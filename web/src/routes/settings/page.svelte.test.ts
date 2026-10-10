// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { SvelteURL } from 'svelte/reactivity';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from '#lib/app.ts';
import { page } from '#lib/testing/app.fake.svelte.ts';
import SettingsPage from './+page.svelte';

vi.mock('$app/state', () => import('#lib/testing/app.fake.svelte.ts'));
vi.mock('$app/navigation', () => import('#lib/testing/app.fake.svelte.ts'));
vi.mock('#lib/app.ts', async () => {
	const { createApi } = await import('#lib/api/client.ts');
	const { createMockBackend } = await import('#lib/api/mock.ts');
	const { SessionStore } = await import('#lib/session.svelte.ts');
	const { EventStore } = await import('#lib/events.svelte.ts');
	const api = createApi({ baseUrl: 'http://fmd2r.test', fetch: createMockBackend().fetch });
	const events = new EventStore({ url: '/api/events', connect: () => ({}) as never });
	return { api, events, session: new SessionStore() };
});

/** The titles of the sections on show. */
const shown = () => screen.getAllByRole('heading', { level: 2 }).map((h) => h.textContent);
const saveBar = () => screen.getByRole('region', { name: 'Save changes' });
const toc = () => screen.getByRole('navigation', { name: 'Settings sections' });

async function open(hash: string) {
	page.url = new SvelteURL(`http://fmd2r.test/settings${hash}`);
	render(SettingsPage);
	await screen.findByRole('navigation', { name: 'Settings sections' });
}

// jsdom lays nothing out, so it cannot scroll.
Element.prototype.scrollIntoView = () => {};

describe('the settings page', () => {
	afterEach(() => (document.body.innerHTML = ''));

	it('shows only the section the URL names, and switches with the table of contents', async () => {
		await open('#section-output');
		expect(shown()).toEqual(['Output']);

		await fireEvent.click(within(toc()).getByRole('link', { name: 'Server' }));
		expect(shown()).toEqual(['Server']);
		expect(page.url.hash).toBe('#section-server');
		expect(within(toc()).getByRole('link', { name: 'Server' }).getAttribute('aria-current')).toBe(
			'location'
		);
	});

	it('keeps an edit in another section, marks that section, and saves it', async () => {
		await open('#section-general');
		await fireEvent.click(screen.getByRole('checkbox', { name: 'Add new downloads stopped' }));

		await fireEvent.click(within(toc()).getByRole('link', { name: 'Output' }));
		expect(shown()).toEqual(['Output']);
		expect(saveBar().textContent).toContain('Unsaved changes');
		const general = within(toc()).getByRole('link', { name: 'General' });
		expect(general.title).toBe('Unsaved changes');
		expect(within(toc()).getByRole('link', { name: 'Output' }).title).toBe('');

		await fireEvent.click(within(saveBar()).getByRole('button', { name: 'Save' }));
		await within(saveBar()).findByText('Saved');
		expect((await api.getSettings()).general.add_as_stopped).toBe(true);
		expect(general.title).toBe('');
	});

	it('switches to the section of a field the server rejected and focuses it', async () => {
		await open('#section-connections');
		const parallel = screen.getByRole('spinbutton', { name: 'Parallel downloads' });
		await fireEvent.input(parallel, { target: { value: '0' } });
		await fireEvent.click(within(toc()).getByRole('link', { name: 'General' }));

		await fireEvent.click(within(saveBar()).getByRole('button', { name: 'Save' }));
		await within(saveBar()).findByText('Fix the highlighted settings to save');
		expect(shown()).toEqual(['Connections']);
		expect(page.url.hash).toBe('#section-connections');
		const field = screen.getByRole('spinbutton', { name: 'Parallel downloads' });
		expect(field.getAttribute('aria-invalid')).toBe('true');
		await waitFor(() => expect(document.activeElement).toBe(field));
	});
});
