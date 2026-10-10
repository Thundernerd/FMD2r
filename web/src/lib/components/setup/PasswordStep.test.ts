// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApi, type Api } from '#lib/api/client.ts';
import { createMockBackend, type MockOptions } from '#lib/api/mock.ts';
import { SETUP_STEPS } from '#lib/setup/wizard.ts';
import SetupWizard from './SetupWizard.svelte';

/** A fresh install's server, reachable from other machines unless `options` say otherwise. */
const serverApi = (options: MockOptions = {}, onUnauthorized = vi.fn()) =>
	createApi({
		baseUrl: 'http://fmd2r.test',
		fetch: createMockBackend({ setUp: false, open: true, ...options }).fetch,
		onUnauthorized
	});

async function renderWizard(api = serverApi(), onfinish = vi.fn()) {
	render(SetupWizard, { api, steps: SETUP_STEPS, onfinish });
	await screen.findByRole('heading', { level: 2 });
	return { api, onfinish };
}

const button = (name: string) => screen.getByRole('button', { name }) as HTMLButtonElement;

/** Opens the setup at its password step, as a reload during setup would. */
async function atPasswordStep(api = serverApi()) {
	await api.patchSettings({ general: { setup_step: 'password' } });
	await renderWizard(api);
	expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('Password');
	return api;
}

async function type(label: string, value: string) {
	await fireEvent.input(screen.getByLabelText(label, { selector: 'input' }), { target: { value } });
}

const stepTitles = () =>
	screen
		.getByRole('navigation', { name: 'Setup steps' })
		.querySelectorAll('li')
		.values()
		.map((li) => li.textContent.trim())
		.toArray();

describe('the setup password step', () => {
	afterEach(() => {
		cleanup();
		sessionStorage.clear();
	});

	it('shows, just before the finish, on a server other machines can reach without a password', async () => {
		await renderWizard();
		expect(stepTitles().slice(-2)).toEqual(['Password', 'Finish']);
	});

	it('is left out on a loopback-only server', async () => {
		await renderWizard(serverApi({ open: false }));
		expect(stepTitles()).not.toContain('Password');
	});

	it('is left out once the server has a password', async () => {
		const api = serverApi();
		await api.patchSettings({ server: { auth_token: 'hunter2' } });
		await api.login('hunter2');
		await renderWizard(api);
		expect(stepTitles()).not.toContain('Password');
	});

	it('is left out when the command line or environment sets the password', async () => {
		await renderWizard(serverApi({ overridden: ['server.auth_token'] }));
		expect(stepTitles()).not.toContain('Password');
	});

	it('explains why it is there', async () => {
		await atPasswordStep();
		expect(screen.getByText(/reachable from other machines/)).toBeTruthy();
	});

	it('keeps Next disabled until the confirmation matches the password', async () => {
		await atPasswordStep();
		await type('Password', 'hunter2');
		await type('Confirm password', 'hunter3');
		expect(button('Next').disabled).toBe(true);

		await type('Confirm password', 'hunter2');
		expect(button('Next').disabled).toBe(false);
	});

	it('sets the password, then logs in with it so the setup goes on', async () => {
		const onUnauthorized = vi.fn();
		const api = await atPasswordStep(serverApi({}, onUnauthorized));
		await type('Password', 'hunter2');
		await type('Confirm password', 'hunter2');
		await fireEvent.click(button('Next'));

		expect(await screen.findByRole('heading', { level: 2, name: 'Finish' })).toBeTruthy();
		expect((await api.health()).auth).toBe(true);
		const settings = await api.getSettings();
		expect(settings.server.has_auth_token).toBe(true);
		expect(settings.general.setup_step).toBe('finish');
		expect(onUnauthorized).not.toHaveBeenCalled();
		expect(await api.login('hunter2')).toBe(true);
	});

	it('leaves the server open when skipped', async () => {
		const api = await atPasswordStep();
		expect(button('Skip').disabled).toBe(false);
		await fireEvent.click(button('Skip'));

		expect(await screen.findByRole('heading', { level: 2, name: 'Finish' })).toBeTruthy();
		expect((await api.health()).auth).toBe(false);
	});

	it('is not logged out by a request the password change itself refused', async () => {
		const backend = createMockBackend({ setUp: false, open: true });
		const onUnauthorized = vi.fn();
		const probes: Promise<unknown>[] = [];
		const api: Api = createApi({
			baseUrl: 'http://fmd2r.test',
			fetch: async (req) => {
				const setsPassword =
					req.method === 'PATCH' && (await req.clone().text()).includes('auth_token');
				const res = await backend.fetch(req);
				// Setting the password closed the event stream, whose reconnect asks the inbox, now
				// without a session.
				if (setsPassword) probes.push(api.listInbox().catch(() => {}));
				return res;
			},
			onUnauthorized
		});
		await atPasswordStep(api);
		await type('Password', 'hunter2');
		await type('Confirm password', 'hunter2');
		await fireEvent.click(button('Next'));

		expect(await screen.findByRole('heading', { level: 2, name: 'Finish' })).toBeTruthy();
		await Promise.all(probes);
		expect(probes).toHaveLength(1);
		expect(onUnauthorized).not.toHaveBeenCalled();
	});

	it('says the password is set when coming Back to it', async () => {
		await atPasswordStep();
		await type('Password', 'hunter2');
		await type('Confirm password', 'hunter2');
		await fireEvent.click(button('Next'));
		await screen.findByRole('heading', { level: 2, name: 'Finish' });

		await fireEvent.click(button('Back'));
		expect(screen.getByText(/A password is set/)).toBeTruthy();
		expect(screen.queryByText(/no password is set/)).toBeNull();
		expect(button('Next').disabled).toBe(false);
	});

	it('resumes at the finish when a password was set elsewhere after leaving off at this step', async () => {
		const api = serverApi();
		await api.patchSettings({ general: { setup_step: 'password' } });
		await api.changePassword('hunter2');
		await renderWizard(api);
		expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('Finish');
	});
});
