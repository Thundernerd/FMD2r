// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';
import type { SetupStep } from '#lib/setup/steps.ts';
import FakeStep from './FakeStep.fixture.svelte';
import SetupWizard from './SetupWizard.svelte';

const STEPS: SetupStep[] = [
	{ id: 'first', title: 'First', component: FakeStep },
	{ id: 'second', title: 'Second', component: FakeStep }
];

/** A fresh install's server. */
const freshApi = () =>
	createApi({ baseUrl: 'http://fmd2r.test', fetch: createMockBackend({ setUp: false }).fetch });

async function open(api = freshApi(), onfinish = vi.fn()) {
	render(SetupWizard, { api, steps: STEPS, onfinish });
	await screen.findByRole('heading', { level: 2 });
	return { api, onfinish };
}

const heading = () => screen.getByRole('heading', { level: 2 }).textContent;
const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;

describe('the setup wizard', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('goes forward with Next, back with Back, and finishes on the last step', async () => {
		const { api, onfinish } = await open();
		expect(heading()).toBe('First');
		expect(screen.getByText('Step 1 of 2')).toBeTruthy();
		expect(screen.queryByRole('button', { name: 'Back' })).toBeNull();

		await fireEvent.input(screen.getByRole('textbox', { name: 'Language' }), {
			target: { value: 'nl' }
		});
		await fireEvent.click(button('Next'));
		expect(await screen.findByText('Step 2 of 2')).toBeTruthy();
		expect(heading()).toBe('Second');
		expect(screen.queryByRole('button', { name: 'Next' })).toBeNull();
		expect((await api.getSettings()).general.language).toBe('nl');

		await fireEvent.click(button('Back'));
		expect(heading()).toBe('First');
		await fireEvent.click(button('Next'));
		await screen.findByText('Step 2 of 2');

		await fireEvent.click(button('Finish'));
		await vi.waitFor(() => expect(onfinish).toHaveBeenCalledWith('/'));
		expect((await api.getSettings()).general.setup_completed).toBe(true);
	});

	it('disables Next while the step says it is not ready', async () => {
		await open();
		await fireEvent.click(screen.getByRole('checkbox', { name: 'Not ready' }));
		expect(button('Next').disabled).toBe(true);

		await fireEvent.click(screen.getByRole('checkbox', { name: 'Not ready' }));
		expect(button('Next').disabled).toBe(false);
	});

	it('keeps the user on a step whose save fails, showing why', async () => {
		await open();
		await fireEvent.click(screen.getByRole('checkbox', { name: 'Invalid value' }));
		await fireEvent.click(button('Next'));

		expect((await screen.findByRole('alert')).textContent).toMatch(/timeout_secs/);
		expect(heading()).toBe('First');
		expect(screen.getByText('Step 1 of 2')).toBeTruthy();
	});

	it('resumes at the first unfinished step with the saved values after a reload', async () => {
		const { api } = await open();
		await fireEvent.input(screen.getByRole('textbox', { name: 'Language' }), {
			target: { value: 'nl' }
		});
		await fireEvent.click(button('Next'));
		await screen.findByText('Step 2 of 2');

		// A reload: the page starts over against the same server.
		cleanup();
		await open(api);
		expect(heading()).toBe('Second');
		await fireEvent.click(button('Back'));
		expect((screen.getByRole('textbox', { name: 'Language' }) as HTMLInputElement).value).toBe(
			'nl'
		);
	});

	it('starts at the first step again once finished', async () => {
		const api = freshApi();
		await api.patchSettings({ general: { setup_completed: true, setup_step: '' } });

		await open(api);
		expect(heading()).toBe('First');
	});

	it('lets a step save and finish the setup, opening another page', async () => {
		const { api, onfinish } = await open();
		await fireEvent.input(screen.getByRole('textbox', { name: 'Language' }), {
			target: { value: 'nl' }
		});
		await fireEvent.click(button('Finish here'));

		await vi.waitFor(() => expect(onfinish).toHaveBeenCalledWith('/settings#section-general'));
		const settings = await api.getSettings();
		expect(settings.general.setup_completed).toBe(true);
		expect(settings.general.language).toBe('nl');
	});
});
