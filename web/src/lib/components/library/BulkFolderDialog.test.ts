// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { Settings } from '#lib/api/types.ts';
import BulkFolderDialog from './BulkFolderDialog.svelte';

beforeAll(() => {
	// jsdom has no modal dialogs.
	HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
		this.open = true;
	};
	HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
		this.open = false;
	};
});

function setup() {
	const api = {
		getSettings: vi.fn(() =>
			Promise.resolve({ saveto: { destinations: [] } } as unknown as Settings)
		)
	};
	const onpick = vi.fn(() => Promise.resolve());
	render(BulkFolderDialog, { api: api as unknown as Api, count: 2, onpick, onclose: vi.fn() });
	return onpick;
}

const BOX = 'Move chapters already downloaded to the new folder';

async function save() {
	await fireEvent.input(screen.getByLabelText('Parent folder'), { target: { value: '/library' } });
	await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
}

describe('BulkFolderDialog', () => {
	it('leaves the chapters already downloaded where they are by default', async () => {
		const onpick = setup();
		expect(screen.getByRole<HTMLInputElement>('checkbox', { name: BOX }).checked).toBe(false);
		expect(screen.queryByText(/they are not moved/)).not.toBeNull();

		await save();

		await waitFor(() => expect(onpick).toHaveBeenCalled());
		expect(onpick).toHaveBeenCalledWith({ destination: '/library' });
	});

	it('moves them in the background when the box is checked', async () => {
		const onpick = setup();
		await fireEvent.click(screen.getByRole('checkbox', { name: BOX }));
		expect(screen.queryByText(/they are not moved/)).toBeNull();
		expect(screen.queryByText(/move to the new folder in the background/)).not.toBeNull();

		await save();

		await waitFor(() => expect(onpick).toHaveBeenCalled());
		expect(onpick).toHaveBeenCalledWith({ destination: '/library', move_files: true });
	});
});
