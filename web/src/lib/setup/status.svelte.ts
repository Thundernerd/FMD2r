import type { Api } from '#lib/api/client.ts';

/** Whether the setup wizard has been finished (`general.setup_completed`). */
export class SetupStatus {
	/** `null` until the settings answer. */
	completed = $state<boolean | null>(null);

	/** Asks the server. When it can't say, the app shows rather than locking the user out. */
	async check(api: Pick<Api, 'getSettings'>) {
		try {
			this.completed = (await api.getSettings()).general.setup_completed;
		} catch {
			this.completed = true;
		}
	}
}

export const setup = new SetupStatus();
