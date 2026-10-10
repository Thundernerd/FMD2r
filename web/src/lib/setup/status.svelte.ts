import { ApiError, type Api } from '#lib/api/client.ts';
import type { Settings } from '#lib/api/types.ts';

/** Whether the setup wizard has been finished (`general.setup_completed`). */
export class SetupStatus {
	/** `null` until the settings answer. */
	completed = $state<boolean | null>(null);

	/**
	 * Asks the server. Without a session it stays unknown until logging in asks again; when the
	 * server can't say otherwise, the app shows rather than locking the user out. Returns the
	 * settings it read, or `null` when it couldn't.
	 */
	async check(api: Pick<Api, 'getSettings'>): Promise<Settings | null> {
		try {
			const settings = await api.getSettings();
			this.completed = settings.general.setup_completed;
			return settings;
		} catch (e) {
			if (!(e instanceof ApiError && e.status === 401)) this.completed = true;
			return null;
		}
	}
}

export const setup = new SetupStatus();
