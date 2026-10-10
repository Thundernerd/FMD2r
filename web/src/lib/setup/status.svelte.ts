import { ApiError, type Api } from '#lib/api/client.ts';

/** Whether the setup wizard has been finished (`general.setup_completed`). */
export class SetupStatus {
	/** `null` until the settings answer. */
	completed = $state<boolean | null>(null);

	/**
	 * Asks the server. Without a session it stays unknown until logging in asks again; when the
	 * server can't say otherwise, the app shows rather than locking the user out.
	 */
	async check(api: Pick<Api, 'getSettings'>) {
		try {
			this.completed = (await api.getSettings()).general.setup_completed;
		} catch (e) {
			if (!(e instanceof ApiError && e.status === 401)) this.completed = true;
		}
	}
}

export const setup = new SetupStatus();
