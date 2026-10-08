import createClient from 'openapi-fetch';
import type { paths } from './schema';
import type { InboxItem, SeriesRef, TaskProgress } from './types';

/** A request the server answered with a non-success status. */
export class ApiError extends Error {
	constructor(
		readonly status: number,
		what: string
	) {
		super(`${what} failed: HTTP ${status}`);
	}
}

/** Everything the UI asks of fmd-server. Pages talk to this, never to `fetch` directly. */
export interface Api {
	listInbox(): Promise<InboxItem[]>;
	markRead(id: string): Promise<void>;
	listTasks(): Promise<TaskProgress[]>;
	/** The series a manga URL points at, or `null` when no module handles the URL. */
	resolveUrl(url: string): Promise<SeriesRef | null>;
}

export interface ApiOptions {
	/** Origin of fmd-server; defaults to the page's own origin. */
	baseUrl?: string;
	fetch?: (input: Request) => Promise<Response>;
}

export function createApi({ baseUrl = '', fetch }: ApiOptions = {}): Api {
	const client = createClient<paths>({ baseUrl, ...(fetch ? { fetch } : {}) });

	const ok = <T>(what: string, res: { data?: T; response: Response }): T => {
		if (!res.response.ok || res.data === undefined) throw new ApiError(res.response.status, what);
		return res.data;
	};

	return {
		async listInbox() {
			return ok('listInbox', await client.GET('/api/inbox'));
		},
		async markRead(id) {
			const { response } = await client.POST('/api/inbox/{id}/read', {
				params: { path: { id } }
			});
			if (!response.ok) throw new ApiError(response.status, 'markRead');
		},
		async listTasks() {
			return ok('listTasks', await client.GET('/api/tasks'));
		},
		async resolveUrl(url) {
			const res = await client.POST('/api/resolve', { body: { url } });
			if (res.response.status === 404) return null;
			return ok('resolveUrl', res);
		}
	};
}
