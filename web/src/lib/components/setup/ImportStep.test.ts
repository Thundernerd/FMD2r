// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import type { SetupStep } from '#lib/setup/steps.ts';
import FakeStep from './FakeStep.fixture.svelte';
import ImportStep from './ImportStep.svelte';
import SetupWizard from './SetupWizard.svelte';

const STEPS: SetupStep[] = [
	{ id: 'import', title: 'Import from FMD2', component: ImportStep },
	{ id: 'next', title: 'Next step', component: FakeStep, paths: ['output.format'] }
];

async function open(steps = STEPS) {
	const api = createApi({
		baseUrl: 'http://fmd2r.test',
		fetch: createMockBackend({ setUp: false }).fetch
	});
	const importFmd2 = vi.spyOn(api, 'importFmd2');
	render(SetupWizard, { api, steps, onfinish: vi.fn() });
	await screen.findByRole('heading', { level: 2 });
	return { api, importFmd2 };
}

/** A file that starts like a zip, as the mock backend checks. */
const USERDATA_ZIP = new File(['PK\u0003\u0004 userdata'], 'userdata.zip', {
	type: 'application/zip'
});
/** The mock import takes a moment, as it reports progress. */
const WAIT = { timeout: 3000 };

const heading = () => screen.getByRole('heading', { level: 2 }).textContent;
const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;

/** Imports the mock userdata from the step: a dry run, then the import. */
async function importUserdata() {
	await fireEvent.click(button('Import'));
	await fireEvent.change(screen.getByLabelText('FMD2 userdata folder, zipped'), {
		target: { files: [USERDATA_ZIP] }
	});
	await fireEvent.click(button('Check'));
	await screen.findByText(/nothing was written/, {}, WAIT);
	await fireEvent.click(button('Import'));
	await screen.findByText('Imported.', {}, WAIT);
}

describe('the import from FMD2 step', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('goes on with Skip without calling the import', async () => {
		const { importFmd2 } = await open();
		expect(heading()).toBe('Import from FMD2');
		expect(screen.getByText(/Coming from FMD2\? Import your library and settings/)).toBeTruthy();

		await fireEvent.click(button('Skip'));
		expect(await screen.findByText('Step 2 of 2')).toBeTruthy();
		expect(heading()).toBe('Next step');
		expect(importFmd2).not.toHaveBeenCalled();
		expect(screen.queryByText(/from your FMD2 settings/)).toBeNull();
	});

	it('starts the later steps from the imported settings, saying so', async () => {
		await open();
		await importUserdata();
		await fireEvent.click(button('Next'));
		await screen.findByText('Step 2 of 2');

		// The mock userdata saves chapters as CBZ (`saveto/Compress` 2).
		expect((screen.getByRole('textbox', { name: 'Format' }) as HTMLInputElement).value).toBe('cbz');
		expect(screen.getByText(/from your FMD2 settings/)).toBeTruthy();
	});

	it('runs a dry run and shows its report before the real import', async () => {
		const { importFmd2 } = await open();
		await fireEvent.click(button('Import'));
		await fireEvent.change(screen.getByLabelText('FMD2 userdata folder, zipped'), {
			target: { files: [USERDATA_ZIP] }
		});
		expect(button('Import').disabled).toBe(true);

		await fireEvent.click(button('Check'));
		expect((await screen.findByRole('status', {}, WAIT)).textContent).toMatch(
			/nothing was written/
		);
		expect(screen.getByRole('table', { name: 'Import report' })).toBeTruthy();
		expect(importFmd2).toHaveBeenCalledTimes(1);
		expect(importFmd2.mock.calls[0]?.[1].dry_run).toBe(true);

		await fireEvent.click(button('Import'));
		await vi.waitFor(
			() => expect(screen.getByRole('status').textContent).toMatch(/Imported/),
			WAIT
		);
		expect(importFmd2).toHaveBeenCalledTimes(2);
		expect(importFmd2.mock.calls[1]?.[1].dry_run).toBe(false);
		// Still on the step, with the report, until the user goes on.
		expect(heading()).toBe('Import from FMD2');
		expect(button('Next').disabled).toBe(false);
	});
});
