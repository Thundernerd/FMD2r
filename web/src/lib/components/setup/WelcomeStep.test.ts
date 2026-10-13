// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import type { SetupStep } from '#lib/setup/steps.ts';
import { EventStore } from '#lib/events.svelte.ts';
import FakeStep from './FakeStep.fixture.svelte';
import SetupWizard from './SetupWizard.svelte';
import WelcomeStep from './WelcomeStep.svelte';

const STEPS: SetupStep[] = [
	{ id: 'welcome', title: 'Welcome', component: WelcomeStep },
	{ id: 'last', title: 'Last step', component: FakeStep }
];

async function open(setUp = false) {
	const api = createApi({
		baseUrl: 'http://fmd2r.test',
		fetch: createMockBackend({ setUp }).fetch
	});
	const restoreBackup = vi.spyOn(api, 'restoreBackup');
	const onfinish = vi.fn();
	const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
	render(SetupWizard, { api, store, steps: STEPS, onfinish });
	await screen.findByRole('heading', { level: 2 });
	return { restoreBackup, onfinish };
}

/** A file that starts like a zip, as the mock backend checks. */
const BACKUP_ZIP = new File(['PK\u0003\u0004 backup'], 'fmd2r-backup-2026-10-13.zip', {
	type: 'application/zip'
});

const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;

/** Opens the restore path and uploads `file`. */
async function restore(file: File) {
	await fireEvent.click(button('Restore from a backup'));
	await fireEvent.change(screen.getByLabelText('FMD2r backup, zipped'), {
		target: { files: [file] }
	});
	await fireEvent.click(button('Restore'));
}

describe('restoring a backup from the welcome step', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('uploads the backup, then says to restart and ends the setup', async () => {
		const { restoreBackup, onfinish } = await open();

		await restore(BACKUP_ZIP);

		expect((await screen.findByRole('status')).textContent).toMatch(
			/Restart FMD2r to finish restoring/
		);
		expect(restoreBackup).toHaveBeenCalledWith(BACKUP_ZIP);
		expect(screen.queryByRole('button', { name: 'Next' })).toBeNull();
		expect(screen.queryByRole('button', { name: 'Restore' })).toBeNull();
		expect(onfinish).not.toHaveBeenCalled();
	});

	it('shows why a backup is refused, and setup goes on', async () => {
		const { restoreBackup } = await open();
		restoreBackup.mockRejectedValueOnce(
			new ApiError(400, 'restoreBackup', 'This backup is from a newer FMD2r; update first.')
		);

		await restore(BACKUP_ZIP);

		expect((await screen.findByRole('alert')).textContent).toMatch(
			'This backup is from a newer FMD2r; update first.'
		);
		expect(screen.queryByText(/Restart FMD2r/)).toBeNull();
		expect(button('Restore').disabled).toBe(false);
		expect(button('Next').disabled).toBe(false);
	});

	it('shows the server’s reason for a file that is no zip', async () => {
		await open();

		await restore(new File(['hello'], 'notes.txt'));

		expect((await screen.findByRole('alert')).textContent).toMatch(/not a zip file/);
	});

	it('is not offered when the server is set up already', async () => {
		await open(true);

		expect(screen.queryByRole('button', { name: 'Restore from a backup' })).toBeNull();
	});
});
