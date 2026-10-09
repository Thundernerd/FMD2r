import type { Api } from '#lib/api/client.ts';
import type { Health } from '#lib/api/types.ts';

/**
 * Whether the server wants the password, and whether this browser lacks a live session: set
 * when any API call answers 401, cleared by logging in.
 */
export class SessionStore {
	/** The server requires the password (`GET /api/health`), so logging out means something. */
	required = $state(false);
	/** The last API call was refused for want of a session: show the login screen. */
	locked = $state(false);
	/** What `GET /api/health` last said; `null` until it answers. */
	health = $state<Health | null>(null);

	/** Asks `GET /api/health` again, e.g. after the password setting changed. */
	async checkHealth(api: Pick<Api, 'health'>) {
		const health = await api.health();
		this.health = health;
		this.required = health.auth;
	}

	/** An API call answered 401, or this browser logged out. */
	unauthorized() {
		this.required = true;
		this.locked = true;
	}

	loggedIn() {
		this.locked = false;
	}

	/** Whether the command line or environment overrides the setting at `path`. */
	overridden(path: string): boolean {
		return this.health?.overridden.includes(path) ?? false;
	}
}
