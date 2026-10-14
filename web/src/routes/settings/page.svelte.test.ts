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
// jsdom has no modal dialogs.
HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
	this.open = true;
};
HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
	this.open = false;
};

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

	it('saves the comic server section, keeping the API key out of the settings', async () => {
		await open('#section-comic_server');
		expect(shown()).toEqual(['Comic server']);
		const kavita = screen.getByRole('option', { name: 'Kavita' }) as HTMLOptionElement;
		await fireEvent.change(screen.getByRole('combobox', { name: 'Server' }), {
			target: { value: kavita.value }
		});
		await fireEvent.input(screen.getByRole('textbox', { name: 'URL' }), {
			target: { value: 'http://nas:5000' }
		});
		await fireEvent.input(screen.getByLabelText('API key'), { target: { value: 'secret-key' } });

		await fireEvent.click(within(saveBar()).getByRole('button', { name: 'Save' }));
		await within(saveBar()).findByText('Saved');
		const saved = (await api.getSettings()).comic_server;
		expect(saved).toEqual({
			kind: 'kavita',
			url: 'http://nas:5000',
			has_api_key: true,
			library_id: null
		});
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

	it('shows why the server rejected a rename template under its field', async () => {
		await open('#section-saveto');
		const filename = screen.getByRole('textbox', { name: 'Page file name' });
		await fireEvent.input(filename, {
			target: { value: '%MANGA% - %CHAPTER - %FILENAME%' }
		});

		await fireEvent.click(within(saveBar()).getByRole('button', { name: 'Save' }));
		await within(saveBar()).findByText('Fix the highlighted settings to save');
		expect(filename.getAttribute('aria-invalid')).toBe('true');
		const describedBy = filename.getAttribute('aria-describedby') ?? '';
		const messages = describedBy
			.split(' ')
			.map((id) => document.getElementById(id)?.textContent ?? '');
		expect(messages).toContain('%CHAPTER is missing its closing %');
	});

	it('offers a backup download from the server in General, with a warning to keep it private', async () => {
		await open('#section-general');

		const link = screen.getByRole('link', { name: 'Download backup' });
		expect(link.getAttribute('href')).toBe('http://fmd2r.test/api/backup');
		expect(link.hasAttribute('download')).toBe(true);
		expect(
			screen.getByText(
				'Includes your library, settings and saved logins. Keep it private: it holds the key to your saved passwords.'
			)
		).toBeTruthy();
	});

	it('saves each notification channel, keeping its secrets write-only', async () => {
		await open('#section-notifications');
		const ntfy = within(screen.getByRole('group', { name: 'ntfy' }));
		await fireEvent.click(ntfy.getByRole('checkbox', { name: 'Send to ntfy' }));
		await fireEvent.input(ntfy.getByRole('textbox', { name: 'Topic URL' }), {
			target: { value: 'https://ntfy.sh/my-topic' }
		});
		await fireEvent.input(ntfy.getByLabelText('Access token'), { target: { value: 'tk' } });
		const discord = within(screen.getByRole('group', { name: 'Discord' }));
		await fireEvent.click(discord.getByRole('checkbox', { name: 'Send to Discord' }));
		await fireEvent.input(discord.getByLabelText('Webhook URL'), {
			target: { value: 'https://discord.com/api/webhooks/1/abc' }
		});
		await fireEvent.click(discord.getByRole('checkbox', { name: 'Problems' }));
		const webhook = within(screen.getByRole('group', { name: 'Webhook' }));
		await fireEvent.click(webhook.getByRole('checkbox', { name: 'Send to Webhook' }));
		await fireEvent.input(webhook.getByRole('textbox', { name: 'URL' }), {
			target: { value: 'http://n8n.local/webhook/fmd' }
		});
		await fireEvent.click(webhook.getByRole('checkbox', { name: 'New chapters' }));

		await fireEvent.click(within(saveBar()).getByRole('button', { name: 'Save' }));
		await within(saveBar()).findByText('Saved');
		const { notifications } = await api.getSettings();
		expect(notifications).toEqual({
			ntfy: {
				enabled: true,
				topic_url: 'https://ntfy.sh/my-topic',
				has_token: true,
				new_chapters: true,
				problems: true
			},
			discord: { enabled: true, has_webhook_url: true, new_chapters: true, problems: false },
			webhook: {
				enabled: true,
				url: 'http://n8n.local/webhook/fmd',
				has_authorization: false,
				new_chapters: false,
				problems: true
			}
		});
	});

	it('offers to repoint the library series in a destination whose folder changed; "Leave them" only saves', async () => {
		await open('#section-saveto');
		const downloads = within(screen.getByRole('group', { name: 'Downloads' }));
		const folder = downloads.getByRole('textbox', { name: 'Folder' });
		// No series downloads to the old folder: nothing to offer.
		await fireEvent.input(folder, { target: { value: '/data/downloads' } });
		await fireEvent.click(within(saveBar()).getByRole('button', { name: 'Save' }));
		await within(saveBar()).findByText('Saved');
		expect(screen.queryByRole('dialog')).toBeNull();

		// The mock library's 8 series download to `/data/downloads/<title>`.
		await fireEvent.input(folder, { target: { value: '/data/library' } });
		await fireEvent.click(within(saveBar()).getByRole('button', { name: 'Save' }));
		const dialog = within(await screen.findByRole('dialog', { name: 'Repoint the library' }));
		const offer = dialog.getByRole('listitem').textContent?.replace(/\s+/g, ' ').trim();
		expect(offer).toBe(
			'8 library series and 0 websites use /data/downloads. Point them to /data/library?'
		);
		const move = dialog.getByRole('checkbox', {
			name: 'Also move their chapters already downloaded'
		}) as HTMLInputElement;
		expect(move.checked).toBe(false);

		await fireEvent.click(dialog.getByRole('button', { name: 'Leave them' }));
		await within(saveBar()).findByText('Saved');
		const { saveto } = await api.getSettings();
		expect(saveto.destinations.map((d) => d.path)).toEqual(['/data/library']);
		const frieren = (await api.listFavorites()).find((f) => f.title === 'Frieren');
		expect(frieren?.save_to).toBe('/data/downloads/Frieren');
	});
});
