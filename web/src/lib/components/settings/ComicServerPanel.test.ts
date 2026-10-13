// @vitest-environment jsdom
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { ComicServerTest, Settings } from '#lib/api/types.ts';
import { Draft } from '#lib/settings/draft.svelte.ts';
import ComicServerPanel from './ComicServerPanel.svelte';

const KOMGA = {
	kind: 'komga',
	url: 'http://nas:25600',
	has_api_key: true,
	library_id: null
} as const;

function setup(result: ComicServerTest | Error) {
	const testComicServer = vi.fn(() =>
		result instanceof Error ? Promise.reject(result) : Promise.resolve(result)
	);
	const api = { testComicServer } as unknown as Api;
	const draft = new Draft({ comic_server: { ...KOMGA } } as unknown as Settings);
	render(ComicServerPanel, { api, draft });
	return { testComicServer, draft };
}

const testButton = () => screen.getByRole('button', { name: 'Test connection' });

describe('the comic server panel', () => {
	it('shows what the server answered and the Komga libraries', async () => {
		setup({
			ok: true,
			message: 'Connected to Komga. Libraries: Manga, Comics.',
			libraries: [
				{ id: 'L1', name: 'Manga' },
				{ id: 'L2', name: 'Comics' }
			]
		});

		await fireEvent.click(testButton());
		const status = await screen.findByRole('status');
		expect(status.textContent).toContain('Connected to Komga. Libraries: Manga, Comics.');
		expect(screen.getByText('L1')).toBeTruthy();
		expect(screen.getByText('L2')).toBeTruthy();
	});

	it('shows why the server could not be reached', async () => {
		setup({
			ok: false,
			message: 'Could not connect to Komga: the server answered 401.',
			libraries: []
		});

		await fireEvent.click(testButton());
		const alert = await screen.findByRole('alert');
		expect(alert.textContent).toContain('the server answered 401');
	});

	it('shows a failed request', async () => {
		setup(new Error('network down'));

		await fireEvent.click(testButton());
		const alert = await screen.findByRole('alert');
		expect(alert.textContent).toContain('network down');
	});

	it('tests the settings as edited, with a typed key', async () => {
		const { testComicServer, draft } = setup({ ok: true, message: 'ok', libraries: [] });
		draft.set('comic_server.kind', 'kavita');
		draft.set('comic_server.url', 'http://nas:5000');
		draft.set('comic_server.api_key', 'typed');

		await fireEvent.click(testButton());
		expect(testComicServer).toHaveBeenCalledWith({
			kind: 'kavita',
			url: 'http://nas:5000',
			api_key: 'typed',
			library_id: null
		});
	});

	it('cannot test without a server', async () => {
		const { draft } = setup({ ok: true, message: 'ok', libraries: [] });
		draft.set('comic_server.kind', 'none');
		await Promise.resolve();
		expect((testButton() as HTMLButtonElement).disabled).toBe(true);
	});
});
