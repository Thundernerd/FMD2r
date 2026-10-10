// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import type { SetupStep } from '#lib/setup/steps.ts';
import FakeStep from './FakeStep.fixture.svelte';
import FormatStep from './FormatStep.svelte';
import SetupWizard from './SetupWizard.svelte';

const STEPS: SetupStep[] = [
	{ id: 'format', title: 'Download format', component: FormatStep },
	{ id: 'next', title: 'Next step', component: FakeStep }
];

async function open() {
	const api = createApi({
		baseUrl: 'http://fmd2r.test',
		fetch: createMockBackend({ setUp: false }).fetch
	});
	const onfinish = vi.fn();
	render(SetupWizard, { api, steps: STEPS, onfinish });
	await screen.findByRole('heading', { level: 2, name: 'Download format' });
	return { api, onfinish };
}

const radio = (name: RegExp) => screen.getByRole('radio', { name }) as HTMLInputElement;

describe('the download format step', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('preselects the current format: a folder of images on a fresh install', async () => {
		await open();
		expect(radio(/^Folder of images/).checked).toBe(true);
		for (const name of [/^ZIP/, /^CBZ/, /^PDF/, /^EPUB/]) expect(radio(name).checked).toBe(false);
	});

	it('explains what each format is for', async () => {
		await open();
		expect(radio(/^CBZ/).labels?.[0]?.textContent).toMatch(/Komga/);
		expect(radio(/^EPUB/).labels?.[0]?.textContent).toMatch(/e-reader/);
	});

	it('saves the picked format on Next', async () => {
		const { api } = await open();
		await fireEvent.click(radio(/^EPUB/));
		await fireEvent.click(screen.getByRole('button', { name: 'Next' }));

		await screen.findByRole('heading', { level: 2, name: 'Next step' });
		expect((await api.getSettings()).output.format).toBe('epub');
	});

	it('links to the other output options in Settings, finishing the setup first', async () => {
		const { api, onfinish } = await open();
		await fireEvent.click(radio(/^CBZ/));
		await fireEvent.click(screen.getByRole('link', { name: /Settings/ }));

		await vi.waitFor(() => expect(onfinish).toHaveBeenCalledWith('/settings#section-output'));
		const settings = await api.getSettings();
		expect(settings.output.format).toBe('cbz');
		expect(settings.general.setup_completed).toBe(true);
	});
});
