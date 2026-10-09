// @vitest-environment jsdom
import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
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
	overridden: []
});

describe('the open server banner', () => {
	afterEach(() => (document.body.innerHTML = ''));

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
});
