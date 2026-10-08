import createClient from 'openapi-fetch';
import type { paths } from './schema';
import type { FacetQuery, SearchQuery } from '#lib/discover/filters.ts';
import type {
	About,
	AccountInfo,
	AccountRequest,
	FavoritePatch,
	FavoriteView,
	InboxItem,
	JobState,
	ListFacets,
	ListJobStarted,
	LogLine,
	ModuleSettingsView,
	ModuleSummary,
	NewTask,
	Problem,
	RenamePreview,
	SaveToSettings,
	SearchPage,
	SeriesInfo,
	SeriesRef,
	Settings,
	TaskDetail,
	TaskSummary
} from './types';

/** A request the server answered with a non-success status. */
export class ApiError extends Error {
	constructor(
		readonly status: number,
		what: string,
		/** The problem's `detail`, when the server said why. */
		readonly detail: string | null = null
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
		super(422, what, detail);
	}
}

/** A JSON merge patch (RFC 7396): changed values, `null` to reset one to its default. */
export type MergePatch =
	paths['/api/settings']['patch']['requestBody']['content']['application/json'];

/** Everything the UI asks of fmd-server. Pages talk to this, never to `fetch` directly. */
export interface Api {
	listInbox(): Promise<InboxItem[]>;
	markRead(id: string): Promise<void>;
	/** The whole download queue, in queue order. */
	listTasks(): Promise<TaskSummary[]>;
	/** A task with its chapters; rejects with status 404 when there is no such task. */
	getTask(id: number): Promise<TaskDetail>;
	/** Runs a task action and resolves to the task as it is then. */
	taskAction(id: number, action: TaskAction): Promise<TaskSummary>;
	startAllTasks(): Promise<void>;
	stopAllTasks(): Promise<void>;
	/** Deletes a task; with `files`, also its chapters' folders and archives. */
	deleteTask(id: number, files: boolean): Promise<void>;
	/** Deletes the finished tasks, keeping their files. */
	removeFinishedTasks(): Promise<void>;
	/** Puts `ids` first in the queue, in that order. */
	reorderTasks(ids: number[]): Promise<void>;
	/** Where a task's files download from (the archive, or a zip of them all). */
	taskFilesUrl(id: number): string;
	/** The series a manga URL points at, or `null` when no module handles the URL. */
	resolveUrl(url: string): Promise<SeriesRef | null>;
	/**
	 * A series' info and chapters; rejects with an {@link ApiError} carrying the server's reason:
	 * 404 when the module finds no series there, 502 when the website cannot be reached.
	 */
	getSeries(module: string, link: string): Promise<SeriesInfo>;
	/** Queues a download; resolves to the new task. */
	createTask(task: NewTask): Promise<TaskSummary>;
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
	/** The library, in library order. */
	listFavorites(): Promise<FavoriteView[]>;
	/**
	 * Adds a series to the library; its current chapters count as seen. Rejects with an
	 * {@link ApiError}: 409 when it is in the library already, 404/502 as {@link getSeries}.
	 */
	addFavorite(module: string, link: string): Promise<FavoriteView>;
	/** Changes the given fields of a favorite. */
	updateFavorite(id: number, patch: FavoritePatch): Promise<FavoriteView>;
	deleteFavorite(id: number): Promise<void>;
	/**
	 * Starts checking the given favorites (every enabled one when omitted) for new chapters;
	 * progress follows as `job.favorites.*` events. Rejects with status 409 while a check runs.
	 */
	checkFavorites(ids?: number[]): Promise<void>;
	/** Starts checking a favorite for chapters missing from its folder; otherwise like {@link checkFavorites}. */
	checkMissingChapters(id: number): Promise<void>;
	/** The accounts of the modules with account support. Passwords are never returned. */
	listAccounts(): Promise<AccountInfo[]>;
	/** Changes the given fields of a module's account; omitted fields keep their value. */
	putAccount(module: string, account: AccountRequest): Promise<AccountInfo>;
	/** Clears a module's credentials and cookies and turns its account off. */
	deleteAccount(module: string): Promise<void>;
	/** Logs in; resolves once the module's login is done. Rejects with 409 while one runs. */
	loginAccount(module: string): Promise<AccountInfo>;
}

/** What a task's action buttons do (`POST /api/tasks/{id}/<action>`). */
export type TaskAction = 'start' | 'stop' | 'redownload' | 'enable' | 'disable';

/** Tasks fetched per request when listing the whole queue (the server's maximum). */
const TASK_PAGE_SIZE = 1000;

export interface ApiOptions {
	/** Origin of fmd-server; defaults to the page's own origin. */
	baseUrl?: string;
	fetch?: (input: Request) => Promise<Response>;
	/** Overrides where task files download from (mock mode has no server to link to). */
	taskFilesUrl?: (id: number) => string;
}

export function createApi({ baseUrl = '', fetch, taskFilesUrl }: ApiOptions = {}): Api {
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
			const tasks: TaskSummary[] = [];
			for (let page = 1; ; page++) {
				const list = unwrap(
					'listTasks',
					await client.GET('/api/tasks', {
						params: { query: { page, per_page: TASK_PAGE_SIZE } }
					})
				);
				tasks.push(...list.items);
				if (list.items.length === 0 || tasks.length >= list.total) return tasks;
			}
		},
		async getTask(id) {
			return unwrap('getTask', await client.GET('/api/tasks/{id}', { params: { path: { id } } }));
		},
		async taskAction(id, action) {
			const params = { params: { path: { id } } };
			const what = `${action}Task`;
			switch (action) {
				case 'start':
					return unwrap(what, await client.POST('/api/tasks/{id}/start', params));
				case 'stop':
					return unwrap(what, await client.POST('/api/tasks/{id}/stop', params));
				case 'redownload':
					return unwrap(what, await client.POST('/api/tasks/{id}/redownload', params));
				case 'enable':
					return unwrap(what, await client.POST('/api/tasks/{id}/enable', params));
				case 'disable':
					return unwrap(what, await client.POST('/api/tasks/{id}/disable', params));
			}
		},
		async startAllTasks() {
			const { response } = await client.POST('/api/tasks/start-all');
			if (!response.ok) throw new ApiError(response.status, 'startAllTasks');
		},
		async stopAllTasks() {
			const { response } = await client.POST('/api/tasks/stop-all');
			if (!response.ok) throw new ApiError(response.status, 'stopAllTasks');
		},
		async deleteTask(id, files) {
			const { response } = await client.DELETE('/api/tasks/{id}', {
				params: { path: { id }, query: { files } }
			});
			if (!response.ok) throw new ApiError(response.status, 'deleteTask');
		},
		async removeFinishedTasks() {
			const { response } = await client.DELETE('/api/tasks', {
				params: { query: { status: 'finished' } }
			});
			if (!response.ok) throw new ApiError(response.status, 'removeFinishedTasks');
		},
		async reorderTasks(ids) {
			const { response } = await client.POST('/api/tasks/reorder', { body: { ids } });
			if (!response.ok) throw new ApiError(response.status, 'reorderTasks');
		},
		taskFilesUrl(id) {
			return taskFilesUrl ? taskFilesUrl(id) : `${baseUrl}/api/tasks/${id}/files`;
		},
		async resolveUrl(url) {
			const res = await client.POST('/api/resolve', { body: { url } });
			if (res.response.status === 404) return null;
			return unwrap('resolveUrl', res);
		},
		async getSeries(module, link) {
			const res = await client.GET('/api/series', { params: { query: { module, link } } });
			if (!res.response.ok || res.data === undefined) {
				// Every error here is a `Problem`.
				const problem = res.error as Partial<Problem> | undefined;
				throw new ApiError(res.response.status, 'getSeries', problem?.detail ?? null);
			}
			return res.data;
		},
		async createTask(task) {
			return validated('createTask', await client.POST('/api/tasks', { body: task }));
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
		async listFavorites() {
			return unwrap('listFavorites', await client.GET('/api/favorites'));
		},
		async addFavorite(module, link) {
			const res = await client.POST('/api/favorites', { body: { module_id: module, link } });
			if (!res.response.ok || res.data === undefined) {
				// Every error here is a `Problem`.
				const problem = res.error as Partial<Problem> | undefined;
				throw new ApiError(res.response.status, 'addFavorite', problem?.detail ?? null);
			}
			return res.data;
		},
		async updateFavorite(id, patch) {
			return validated(
				'updateFavorite',
				await client.PATCH('/api/favorites/{id}', { params: { path: { id } }, body: patch })
			);
		},
		async deleteFavorite(id) {
			const { response } = await client.DELETE('/api/favorites/{id}', {
				params: { path: { id } }
			});
			if (!response.ok) throw new ApiError(response.status, 'deleteFavorite');
		},
		async checkFavorites(ids) {
			const { response } = await client.POST('/api/favorites/check', {
				body: ids ? { ids } : {}
			});
			if (!response.ok) throw new ApiError(response.status, 'checkFavorites');
		},
		async checkMissingChapters(id) {
			const { response } = await client.POST('/api/favorites/{id}/check-missing', {
				params: { path: { id } }
			});
			if (!response.ok) throw new ApiError(response.status, 'checkMissingChapters');
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
