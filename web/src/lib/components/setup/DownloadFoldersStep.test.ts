// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import type { SetupStep } from '#lib/setup/steps.ts';
import DownloadFoldersStep from './DownloadFoldersStep.svelte';
import FakeStep from './FakeStep.fixture.svelte';
import SetupWizard from './SetupWizard.svelte';

const STEPS: SetupStep[] = [
	{ id: 'download-folders', title: 'Download folders', component: DownloadFoldersStep },
	{ id: 'next', title: 'Next step', component: FakeStep }
];

async function open({ inContainer = false } = {}) {
	const api = createApi({
		baseUrl: 'http://fmd2r.test',
		fetch: createMockBackend({ setUp: false, inContainer }).fetch
	});
	render(SetupWizard, { api, steps: STEPS, onfinish: () => {} });
	await screen.findByRole('heading', { level: 2, name: 'Download folders' });
	return api;
}

const row = (name: string) => screen.getByRole('group', { name });
const next = () => screen.getByRole('button', { name: 'Next' }) as HTMLButtonElement;

async function addDestination(name: string, path: string) {
	await fireEvent.click(screen.getByRole('button', { name: 'Add destination' }));
	const added = row('New destination');
	await fireEvent.input(within(added).getByRole('textbox', { name: 'Folder' }), {
		target: { value: path }
	});
	await fireEvent.input(within(added).getByRole('textbox', { name: 'Name' }), {
		target: { value: name }
	});
}

describe('the download folders setup step', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('starts with the default destination', async () => {
		await open();
		const downloads = row('Downloads');
		expect(
			(within(downloads).getByRole('textbox', { name: 'Folder' }) as HTMLInputElement).value
		).toBe('downloads');
		expect(
			(within(downloads).getByRole('radio', { name: 'Default' }) as HTMLInputElement).checked
		).toBe(true);
	});

	it('saves every destination on Next, with the default chosen', async () => {
		const api = await open();
		await addDestination('Manhwa', '/data/manhwa');
		await fireEvent.click(within(row('Manhwa')).getByRole('radio', { name: 'Default' }));
		await fireEvent.click(next());

		await screen.findByRole('heading', { level: 2, name: 'Next step' });
		expect((await api.getSettings()).saveto.destinations).toEqual([
			{ name: 'Downloads', path: 'downloads', default: false },
			{ name: 'Manhwa', path: '/data/manhwa', default: true }
		]);
	});

	it('keeps Next disabled while two destinations share a name', async () => {
		await open();
		await addDestination('downloads', '/data/manhwa');
		expect(next().disabled).toBe(true);
		expect(screen.getByRole('alert').textContent).toMatch(/another destination is named downloads/);

		await fireEvent.input(within(row('downloads')).getByRole('textbox', { name: 'Name' }), {
			target: { value: 'Manhwa' }
		});
		expect(next().disabled).toBe(false);
	});

	it('says that folders must be mounted when the server runs in a container', async () => {
		await open({ inContainer: true });
		expect(await screen.findByText(/each folder must be mounted into it/)).toBeTruthy();
	});

	it('says nothing about mounting folders outside a container', async () => {
		await open();
		await new Promise((resolve) => setTimeout(resolve, 0));
		expect(screen.queryByText(/must be mounted/)).toBeNull();
	});
});
