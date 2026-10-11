// @vitest-environment jsdom
import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { Health } from '#lib/api/types.ts';
import { SessionStore } from '#lib/session.svelte.ts';
import OpenServerBanner from './OpenServerBanner.svelte';

/** The session after `GET /api/health` answered `health`. */
async function sessionWith(health: Health) {
	const session = new SessionStore();
	const api: Pick<Api, 'health'> = { health: () => Promise.resolve(health) };
	await session.checkHealth(api);
	return session;
}

const health = (auth: boolean, loopback: boolean): Health => ({
	status: 'ok',
	auth,
	loopback,
	in_container: false,
	overridden: []
});

describe('the open server banner', () => {
	beforeEach(() => localStorage.clear());
	afterEach(() => {
		document.body.innerHTML = '';
		vi.restoreAllMocks();
	});

	it('warns when other machines can reach a server without a password', async () => {
		const session = await sessionWith(health(false, false));
		render(OpenServerBanner, { health: session.health });
		expect(screen.getByRole('alert').textContent).toContain('no password');
	});

	it('stays away with a password or on a loopback address', async () => {
		for (const h of [health(true, false), health(false, true)]) {
			const session = await sessionWith(h);
			render(OpenServerBanner, { health: session.health });
			expect(screen.queryByRole('alert')).toBeNull();
		}
	});

	it('stays dismissed in this browser', async () => {
		const session = await sessionWith(health(false, false));
		const { unmount } = render(OpenServerBanner, { health: session.health });
		await fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
		expect(screen.queryByRole('alert')).toBeNull();
		unmount();

		render(OpenServerBanner, { health: session.health });
		expect(screen.queryByRole('alert')).toBeNull();
	});

	it('comes back once the server was secured and then opened again', async () => {
		const open = await sessionWith(health(false, false));
		const first = render(OpenServerBanner, { health: open.health });
		await fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
		first.unmount();

		const secured = await sessionWith(health(true, false));
		render(OpenServerBanner, { health: secured.health }).unmount();

		render(OpenServerBanner, { health: open.health });
		expect(screen.getByRole('alert').textContent).toContain('no password');
	});

	it('still warns, and can still be dismissed, when this browser refuses storage', async () => {
		const refuse = () => {
			throw new DOMException('denied', 'SecurityError');
		};
		vi.spyOn(Storage.prototype, 'getItem').mockImplementation(refuse);
		vi.spyOn(Storage.prototype, 'setItem').mockImplementation(refuse);
		vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(refuse);

		const secured = await sessionWith(health(true, false));
		render(OpenServerBanner, { health: secured.health }).unmount();

		const open = await sessionWith(health(false, false));
		render(OpenServerBanner, { health: open.health });
		expect(screen.getByRole('alert').textContent).toContain('no password');
		await fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
		expect(screen.queryByRole('alert')).toBeNull();
	});
});
