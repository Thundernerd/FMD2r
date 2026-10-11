// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import ImportFlow from './ImportFlow.svelte';

function open() {
	const api = createApi({ baseUrl: 'http://fmd2r.test', fetch: createMockBackend().fetch });
	render(ImportFlow, { api, job: undefined, onimported: vi.fn() });
}

const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;

async function choose(file: File) {
	await fireEvent.change(screen.getByLabelText('FMD2 userdata folder, zipped'), {
		target: { files: [file] }
	});
}

describe('the FMD2 import flow', () => {
	afterEach(cleanup);

	it('disables Check and Import until a file is chosen', () => {
		open();
		expect(button('Check').disabled).toBe(true);
		expect(button('Import').disabled).toBe(true);
	});

	it('enables Check, but not Import, for a file not yet checked', async () => {
		open();
		await choose(new File(['zip'], 'userdata.zip', { type: 'application/zip' }));
		expect(button('Check').disabled).toBe(false);
		expect(button('Import').disabled).toBe(true);
	});
});
