// @vitest-environment jsdom
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Destination, FolderCheck } from '#lib/api/types.ts';
import { Draft } from '#lib/settings/draft.svelte.ts';
import DestinationsEditor from './DestinationsEditor.svelte';

const MANGA: Destination = { name: 'Manga', path: '/data/manga', default: true };
const MANHWA: Destination = { name: 'Manhwa', path: '/data/manhwa', default: false };

/** Every folder is fine except `/mnt/usb`, whose disk is unmounted. */
const checkFolders = vi.fn(async (paths: string[]): Promise<FolderCheck[]> =>
	paths.map((path) => ({
		path,
		problem: path === '/mnt/usb' ? 'the folder does not exist' : null
	}))
);

function renderEditor(destinations: Destination[]) {
	const draft = new Draft<object>({
		saveto: { default_dir: destinations.find((d) => d.default)?.path, destinations }
	});
	render(DestinationsEditor, { draft, checkFolders });
	return draft;
}

const destinations = (draft: Draft<object>) => draft.get('saveto.destinations');
const row = (name: string) => screen.getByRole('group', { name });

describe('DestinationsEditor', () => {
	afterEach(() => (document.body.innerHTML = ''));

	it('adds a destination', async () => {
		const draft = renderEditor([MANGA]);
		await fireEvent.click(screen.getByRole('button', { name: 'Add destination' }));
		const added = row('New destination');
		await fireEvent.input(within(added).getByRole('textbox', { name: 'Folder' }), {
			target: { value: '/data/manhwa' }
		});
		expect(destinations(draft)).toEqual([
			MANGA,
			{ name: 'New destination', path: '/data/manhwa', default: false }
		]);
	});

	it('renames a destination', async () => {
		const draft = renderEditor([MANGA, MANHWA]);
		await fireEvent.input(within(row('Manhwa')).getByRole('textbox', { name: 'Name' }), {
			target: { value: 'Webtoons' }
		});
		expect(destinations(draft)).toEqual([MANGA, { ...MANHWA, name: 'Webtoons' }]);
	});

	it('removes a destination but not the default', async () => {
		const draft = renderEditor([MANGA, MANHWA]);
		const removeDefault = within(row('Manga')).getByRole('button', { name: 'Remove' });
		expect((removeDefault as HTMLButtonElement).disabled).toBe(true);
		expect(removeDefault.getAttribute('title')).toBe("The default destination can't be removed");

		await fireEvent.click(within(row('Manhwa')).getByRole('button', { name: 'Remove' }));
		expect(destinations(draft)).toEqual([MANGA]);
	});

	it('makes another destination the default, which can then be removed', async () => {
		const draft = renderEditor([MANGA, MANHWA]);
		await fireEvent.click(within(row('Manhwa')).getByRole('radio', { name: 'Default' }));
		expect(destinations(draft)).toEqual([
			{ ...MANGA, default: false },
			{ ...MANHWA, default: true }
		]);
		await fireEvent.click(within(row('Manga')).getByRole('button', { name: 'Remove' }));
		expect(destinations(draft)).toEqual([{ ...MANHWA, default: true }]);
	});

	it('moves a destination up and down', async () => {
		const draft = renderEditor([MANGA, MANHWA]);
		await fireEvent.click(within(row('Manhwa')).getByRole('button', { name: 'Move up' }));
		expect(destinations(draft)).toEqual([MANHWA, MANGA]);
		await fireEvent.click(within(row('Manhwa')).getByRole('button', { name: 'Move down' }));
		expect(destinations(draft)).toEqual([MANGA, MANHWA]);
	});

	it('warns about a folder that is missing, without blocking the save', async () => {
		const draft = renderEditor([MANGA, { name: 'USB', path: '/mnt/usb', default: false }]);
		expect(await within(row('USB')).findByText(/the folder does not exist/)).toBeTruthy();
		expect(within(row('Manga')).queryByText(/does not exist/)).toBeNull();
		expect(draft.errors).toEqual({});
	});

	it('shows the errors the server reported on a destination', () => {
		const draft = renderEditor([MANGA, { name: 'manga', path: '', default: false }]);
		draft.errors['saveto.destinations.1.name'] = 'another destination is named manga';
		draft.errors['saveto.destinations.1.path'] = 'a destination needs a folder';
		return Promise.resolve().then(() => {
			const second = row('manga');
			expect(within(second).getByText('another destination is named manga')).toBeTruthy();
			expect(within(second).getByText('a destination needs a folder')).toBeTruthy();
		});
	});
});
