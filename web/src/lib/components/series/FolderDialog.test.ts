// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { FavoriteView, Settings } from '#lib/api/types.ts';
import FolderDialog from './FolderDialog.svelte';

beforeAll(() => {
	// jsdom has no modal dialogs.
	HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
		this.open = true;
	};
	HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
		this.open = false;
	};
});

const FAVORITE = {
	id: 7,
	module_id: 't',
	title: 'Alpha',
	save_to: '/old/Alpha'
} as FavoriteView;

function setup() {
	const api = {
		getSettings: vi.fn(() =>
			Promise.resolve({ saveto: { destinations: [] } } as unknown as Settings)
		),
		saveFolder: vi.fn(() => Promise.resolve('')),
		updateFavorite: vi.fn((_id: number, patch: { save_to?: string }) =>
			Promise.resolve({ ...FAVORITE, save_to: patch.save_to ?? FAVORITE.save_to })
		)
	};
	render(FolderDialog, {
		api: api as unknown as Api,
		favorite: FAVORITE,
		onsaved: vi.fn(),
		onclose: vi.fn()
	});
	return api;
}

async function changeFolder() {
	await fireEvent.input(screen.getByLabelText('Folder'), { target: { value: '/new/Alpha' } });
}

describe('FolderDialog', () => {
	it('leaves the chapters already downloaded where they are by default', async () => {
		const api = setup();
		const box = screen.getByRole<HTMLInputElement>('checkbox', {
			name: 'Move chapters already downloaded to the new folder'
		});
		expect(box.checked).toBe(false);
		expect(screen.queryByText(/they are not moved/)).not.toBeNull();

		await changeFolder();
		await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

		await waitFor(() => expect(api.updateFavorite).toHaveBeenCalled());
		expect(api.updateFavorite).toHaveBeenCalledWith(7, { save_to: '/new/Alpha' });
	});

	it('moves them in the background when the box is checked', async () => {
		const api = setup();
		await fireEvent.click(
			screen.getByRole('checkbox', { name: 'Move chapters already downloaded to the new folder' })
		);
		expect(screen.queryByText(/they are not moved/)).toBeNull();
		expect(screen.queryByText(/move to the new folder in the background/)).not.toBeNull();

		await changeFolder();
		await fireEvent.click(screen.getByRole('button', { name: 'Save' }));

		await waitFor(() => expect(api.updateFavorite).toHaveBeenCalled());
		expect(api.updateFavorite).toHaveBeenCalledWith(7, {
			save_to: '/new/Alpha',
			move_files: true
		});
	});
});
