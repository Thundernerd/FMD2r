import createClient from 'openapi-fetch';
import type { paths } from './schema';
import type { FacetQuery, SearchQuery } from '#lib/discover/filters.ts';
import type {
	About,
	AccountInfo,
	AccountRequest,
	InboxItem,
	JobState,
	ListFacets,
	ListJobStarted,
	LogLine,
	ModuleSettingsView,
	ModuleSummary,
	Problem,
	RenamePreview,
	SaveToSettings,
	SearchPage,
	SeriesRef,
	Settings,
	TaskProgress
} from './types';

/** A request the server answered with a non-success status. */
export class ApiError extends Error {
	constructor(
		readonly status: number,
		what: string
	) {
		super(`${what} failed: HTTP ${status}`);
	}
}

/** An update the server rejected as invalid (422); `field` names the offending setting. */
export class ValidationError extends ApiError {
	constructor(
		readonly field: string | null,
		readonly detail: string,
		what: string
	) {
		super(422, what);
	}
}

/** A JSON merge patch (RFC 7396): changed values, `null` to reset one to its default. */
export type MergePatch =
	paths['/api/settings']['patch']['requestBody']['content']['application/json'];

/** Everything the UI asks of fmd-server. Pages talk to this, never to `fetch` directly. */
export interface Api {
	listInbox(): Promise<InboxItem[]>;
	markRead(id: string): Promise<void>;
	listTasks(): Promise<TaskProgress[]>;
	/** The series a manga URL points at, or `null` when no module handles the URL. */
	resolveUrl(url: string): Promise<SeriesRef | null>;
	/** The server's buffered log lines, oldest first. */
	listLogs(): Promise<LogLine[]>;
	listJobs(): Promise<JobState[]>;
	/** Starts a job; rejects with status 409 when it already runs. */
	runJob(id: string): Promise<JobState>;
	/** Cancels a running job; rejects with status 409 when it does not run. */
	cancelJob(id: string): Promise<JobState>;
	/** Server diagnostics; runs the tool checks, so it can take a few seconds. */
	about(): Promise<About>;
	getSettings(): Promise<Settings>;
	/** Applies `patch`; rejects with a {@link ValidationError} naming the field when invalid. */
	patchSettings(patch: MergePatch): Promise<Settings>;
	/** The names a draft's rename templates produce for a sample series. */
	previewRename(saveto: SaveToSettings): Promise<RenamePreview>;
	listModules(): Promise<ModuleSummary[]>;
	getModuleSettings(id: string): Promise<ModuleSettingsView>;
	/** Applies `patch`; rejects with a {@link ValidationError} naming the field when invalid. */
	patchModuleSettings(id: string, patch: MergePatch): Promise<ModuleSettingsView>;
	/** One page of the manga lists matching `query`. */
	searchLists(query: SearchQuery): Promise<SearchPage>;
	/** Genre and status counts of the titles `query` matches. */
	listFacets(query: FacetQuery): Promise<ListFacets>;
	/**
	 * Starts updating a module's list from its website; progress follows as `job.lists.*`
	 * events. Rejects with status 409 when a list job of the module runs.
	 */
	updateList(module: string): Promise<ListJobStarted>;
	/** Starts replacing a module's list with its FMD2-DB dump; otherwise like {@link updateList}. */
	importListDb(module: string): Promise<ListJobStarted>;
	/** Stops a module's list job; rejects with status 409 when none runs. */
	cancelListJob(module: string): Promise<void>;
	/** The accounts of the modules with account support. Passwords are never returned. */
	listAccounts(): Promise<AccountInfo[]>;
	/** Changes the given fields of a module's account; omitted fields keep their value. */
	putAccount(module: string, account: AccountRequest): Promise<AccountInfo>;
	/** Clears a module's credentials and cookies and turns its account off. */
	deleteAccount(module: string): Promise<void>;
	/** Logs in; resolves once the module's login is done. Rejects with 409 while one runs. */
	loginAccount(module: string): Promise<AccountInfo>;
}

export interface ApiOptions {
	/** Origin of fmd-server; defaults to the page's own origin. */
	baseUrl?: string;
	fetch?: (input: Request) => Promise<Response>;
}

export function createApi({ baseUrl = '', fetch }: ApiOptions = {}): Api {
	const client = createClient<paths>({ baseUrl, ...(fetch ? { fetch } : {}) });

	const unwrap = <T>(what: string, res: { data?: T; response: Response }): T => {
		if (!res.response.ok || res.data === undefined) throw new ApiError(res.response.status, what);
		return res.data;
	};

	/** Like `unwrap`, but turns a 422 problem into a {@link ValidationError}. */
	const validated = <T>(
		what: string,
		res: { data?: T; error?: unknown; response: Response }
	): T => {
		if (res.response.status === 422) {
			// The problem body is typed per operation; every 422 here is a `Problem`.
			const problem = res.error as Partial<Problem> | undefined;
			throw new ValidationError(problem?.field ?? null, problem?.detail ?? 'Invalid value.', what);
		}
		return unwrap(what, res);
	};

	return {
		async listInbox() {
			return unwrap('listInbox', await client.GET('/api/inbox'));
		},
		async markRead(id) {
			const { response } = await client.POST('/api/inbox/{id}/read', {
				params: { path: { id } }
			});
			if (!response.ok) throw new ApiError(response.status, 'markRead');
		},
		async listTasks() {
			return unwrap('listTasks', await client.GET('/api/tasks'));
		},
		async resolveUrl(url) {
			const res = await client.POST('/api/resolve', { body: { url } });
			if (res.response.status === 404) return null;
			return unwrap('resolveUrl', res);
		},
		async listLogs() {
			return unwrap('listLogs', await client.GET('/api/logs'));
		},
		async listJobs() {
			return unwrap('listJobs', await client.GET('/api/jobs'));
		},
		async runJob(id) {
			return unwrap(
				'runJob',
				await client.POST('/api/jobs/{id}/run', { params: { path: { id } } })
			);
		},
		async cancelJob(id) {
			return unwrap(
				'cancelJob',
				await client.POST('/api/jobs/{id}/cancel', { params: { path: { id } } })
			);
		},
		async about() {
			return unwrap('about', await client.GET('/api/about'));
		},
		async getSettings() {
			return unwrap('getSettings', await client.GET('/api/settings'));
		},
		async patchSettings(patch) {
			return validated('patchSettings', await client.PATCH('/api/settings', { body: patch }));
		},
		async previewRename(saveto) {
			return unwrap('previewRename', await client.POST('/api/preview-rename', { body: saveto }));
		},
		async listModules() {
			return unwrap('listModules', await client.GET('/api/modules'));
		},
		async getModuleSettings(id) {
			return unwrap(
				'getModuleSettings',
				await client.GET('/api/modules/{id}/settings', { params: { path: { id } } })
			);
		},
		async patchModuleSettings(id, patch) {
			return validated(
				'patchModuleSettings',
				await client.PATCH('/api/modules/{id}/settings', {
					params: { path: { id } },
					body: patch
				})
			);
		},
		async searchLists(query) {
			return unwrap('searchLists', await client.GET('/api/lists/search', { params: { query } }));
		},
		async listFacets(query) {
			return unwrap('listFacets', await client.GET('/api/lists/facets', { params: { query } }));
		},
		async updateList(module) {
			return unwrap(
				'updateList',
				await client.POST('/api/lists/{module}/update', { params: { path: { module } } })
			);
		},
		async importListDb(module) {
			return unwrap(
				'importListDb',
				await client.POST('/api/lists/{module}/import-db', { params: { path: { module } } })
			);
		},
		async cancelListJob(module) {
			const { response } = await client.POST('/api/lists/{module}/cancel', {
				params: { path: { module } }
			});
			if (!response.ok) throw new ApiError(response.status, 'cancelListJob');
		},
		async listAccounts() {
			return unwrap('listAccounts', await client.GET('/api/accounts'));
		},
		async putAccount(module, account) {
			return unwrap(
				'putAccount',
				await client.PUT('/api/accounts/{module}', {
					params: { path: { module } },
					body: account
				})
			);
		},
		async deleteAccount(module) {
			const { response } = await client.DELETE('/api/accounts/{module}', {
				params: { path: { module } }
			});
			if (!response.ok) throw new ApiError(response.status, 'deleteAccount');
		},
		async loginAccount(module) {
			return unwrap(
				'loginAccount',
				await client.POST('/api/accounts/{module}/login', { params: { path: { module } } })
			);
		}
	};
}
