import { getPath, isObject } from '#lib/settings/draft.svelte.ts';
import { SETTINGS_SECTIONS } from '#lib/settings/sections.ts';
import type {
	ModuleOptionSetting,
	ModuleSettingsView,
	ModuleSummary,
	RenamePreview,
	SaveToSettings,
	Settings
} from './types';

// The settings half of the mock backend: the T18 defaults, a few modules with `AddOption*`
// options, merge-patch updates with the server's range checks, and a rough rename preview.
// State lives in sessionStorage so a reload keeps what was saved, like the real server.

const DEFAULT_USER_AGENT =
	'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36';

/** The server's defaults (fmd_core::settings::Settings::default()). */
export const defaultSettings = (): Settings => ({
	general: {
		data_dir: 'data',
		lua_dir: 'lua',
		language: 'en',
		add_as_stopped: false,
		load_covers: true
	},
	connections: {
		max_parallel_tasks: 1,
		threads_per_task: 1,
		retry_count: 5,
		auto_retry_failed_tasks: 1,
		always_start_from_failed_chapters: true,
		max_favorite_threads: 1,
		max_update_list_threads: 1,
		timeout_secs: 30,
		user_agent: DEFAULT_USER_AGENT,
		proxy: { enabled: false, type: 'http', host: '', port: null, username: '', password: '' }
	},
	saveto: {
		default_dir: 'downloads',
		generate_manga_folder: true,
		manga_rename: '%MANGA%',
		generate_chapter_folder: true,
		chapter_rename: '%CHAPTER%',
		filename_rename: '%FILENAME%',
		remove_manga_name_from_chapter: false,
		replace_unicode: false,
		replace_unicode_with: '_',
		convert_digit_volume: true,
		digit_volume_length: 2,
		convert_digit_chapter: true,
		digit_chapter_length: 3,
		illegal_chars: 'posix'
	},
	output: { format: 'folder', pdf_quality: 100 },
	images: {
		png_to_jpeg: false,
		webp_save_as: 'png',
		png_compression: 'fastest',
		jpeg_quality: 80,
		imagemagick: { enabled: false, save_as: 'JPEG', compression: 'None', quality: 75 }
	},
	favorites: {
		check_at_startup: true,
		check_on_interval: true,
		check_interval_minutes: 60,
		auto_download: false,
		remove_completed: false
	},
	update_lists: {
		auto_update: false,
		interval_hours: 24,
		no_manga_info: false,
		remove_duplicate_local_data: false,
		new_manga_days: 1,
		db_url: 'https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/<website>.7z'
	},
	module_updater: {
		auto_update: true,
		interval_minutes: 60,
		repo_owner: 'dazedcat19',
		repo_name: 'FMD2',
		repo_ref: 'master',
		repo_path: 'lua',
		github_token: null,
		keep_last_good: true
	},
	server: { bind: '0.0.0.0:8080', auth_token: null },
	xpath: { backend: 'fpc' }
});

/** A module as the mock knows it: what it declares. Values live in the module's overrides. */
interface MockModule {
	summary: Omit<ModuleSummary, 'option_count'>;
	limits: ModuleSettingsView['module_limits'];
	options: ModuleOptionSetting[];
}

const LANGUAGES = ['All', 'English', 'Japanese', 'Spanish (LATAM)', 'Indonesian', 'French'];

const MODULES: MockModule[] = [
	{
		summary: { id: 'batoto', name: 'Bato.to', category: 'English' },
		limits: { max_task_limit: 0, max_thread_per_task_limit: 0, max_connection_limit: 0 },
		options: []
	},
	{
		summary: { id: 'comick', name: 'ComicK', category: 'English' },
		limits: { max_task_limit: 1, max_thread_per_task_limit: 2, max_connection_limit: 2 },
		options: [
			{
				kind: 'edit',
				key: 'luagroups',
				caption: 'Preferred scanlation groups',
				default: '',
				value: ''
			}
		]
	},
	{
		summary: { id: 'mangadex', name: 'MangaDex', category: 'English' },
		limits: { max_task_limit: 0, max_thread_per_task_limit: 0, max_connection_limit: 4 },
		// lua/modules/MangaDex.lua:45-53.
		options: [
			{
				kind: 'spinedit',
				key: 'mdx_delay',
				caption: 'Delay (s) between requests',
				default: 1,
				value: 1,
				min: 0,
				max: 10000
			},
			{
				kind: 'checkbox',
				key: 'luashowscangroup',
				caption: 'Show scanlation group',
				default: false,
				value: false
			},
			{
				kind: 'checkbox',
				key: 'luashowchaptertitle',
				caption: 'Show chapter title',
				default: true,
				value: true
			},
			{
				kind: 'checkbox',
				key: 'luadatasaver',
				caption: 'Data saver',
				default: false,
				value: false
			},
			{
				kind: 'combobox',
				key: 'lualang',
				caption: 'Language',
				items: LANGUAGES,
				default: 1,
				value: 1
			}
		]
	},
	{
		summary: { id: 'webtoons', name: 'Webtoons', category: 'English' },
		limits: { max_task_limit: 0, max_thread_per_task_limit: 0, max_connection_limit: 0 },
		options: []
	}
];

type Overrides = Pick<ModuleSettingsView, 'enabled' | 'limits' | 'http'> & {
	options: Record<string, unknown>;
};

const defaultOverrides = (): Overrides => ({
	enabled: false,
	limits: { max_task_limit: 0, max_thread_per_task_limit: 0, max_connection_limit: 0 },
	http: {
		user_agent: '',
		cookies: '',
		proxy: { type: 'default', host: '', port: '', username: '', password: '' }
	},
	options: {}
});

type JsonObject = Record<string, unknown>;

/** A 422 the way fmd-server reports it. */
export class Invalid extends Error {
	constructor(
		readonly field: string,
		readonly detail: string
	) {
		super(detail);
	}
}

/**
 * RFC 7396 merge of `patch` into `target`, rejecting unknown keys like the server does. `null`
 * resets a value to the one in `defaults`.
 */
function mergePatch(target: JsonObject, patch: JsonObject, defaults: JsonObject, path = '') {
	for (const [key, value] of Object.entries(patch)) {
		const field = path ? `${path}.${key}` : key;
		if (!(key in target)) throw new Invalid(field, `unknown setting ${field}`);
		const current = target[key];
		const fallback = defaults[key];
		if (value === null) target[key] = structuredClone(fallback);
		else if (isObject(current) && isObject(value) && isObject(fallback))
			mergePatch(current, value, fallback, field);
		else target[key] = value;
	}
}

/** The server's range checks, as the settings page's own field definitions state them. */
function validateSettings(settings: Settings) {
	for (const field of SETTINGS_SECTIONS.flatMap((s) => s.fields)) {
		const value = getPath(settings, field.path);
		const control = field.control;
		if (control.kind === 'number') {
			if (value === null && control.nullable) continue;
			if (typeof value !== 'number' || !Number.isInteger(value))
				throw new Invalid(field.path, 'expected an integer');
			if (value < control.min || value > control.max)
				throw new Invalid(field.path, `${value} is outside ${control.min}..=${control.max}`);
		}
	}
}

function checkOption(option: ModuleOptionSetting, value: unknown): string | null {
	switch (option.kind) {
		case 'checkbox':
			return typeof value === 'boolean' ? null : 'expected true or false';
		case 'edit':
			return typeof value === 'string' ? null : 'expected a string';
		case 'spinedit':
			return Number.isInteger(value) && Number(value) >= option.min && Number(value) <= option.max
				? null
				: `expected an integer in ${option.min}..=${option.max}`;
		case 'combobox':
			return Number.isInteger(value) && Number(value) >= 0 && Number(value) < option.items.length
				? null
				: `expected the index of an item, 0..=${option.items.length - 1}`;
	}
}

const STORAGE_KEY = 'fmd2r-mock-settings';

interface Stored {
	settings: Settings;
	modules: Record<string, Overrides>;
}

function load(): Stored {
	try {
		const raw = globalThis.sessionStorage?.getItem(STORAGE_KEY);
		if (raw) return JSON.parse(raw) as Stored;
	} catch {
		// No storage (tests, private mode): start from the defaults.
	}
	return { settings: defaultSettings(), modules: {} };
}

/** Rough `CustomRename`: substitutes the tokens, no padding or symbol rules. */
function previewRename(saveto: SaveToSettings): RenamePreview {
	const fill = (template: string, chapter: string) =>
		template
			.replaceAll('%WEBSITE%', 'MangaDex')
			.replaceAll('%MANGA%', 'Sample Manga')
			.replaceAll('%AUTHOR%', 'Sample Author')
			.replaceAll('%ARTIST%', 'Sample Artist')
			.replaceAll('%CHAPTER%', chapter)
			.replaceAll('%NUMBERING%', chapter ? '0005' : '')
			.trim();
	return {
		manga: fill(saveto.manga_rename || '%MANGA%', ''),
		chapter: fill(saveto.chapter_rename || '%CHAPTER%', 'Vol. 01 Ch. 005'),
		filename: (saveto.filename_rename || '%FILENAME%').replaceAll('%FILENAME%', '001')
	};
}

/** The mock's settings state and operations. */
export function createMockSettings() {
	const state = load();
	const save = () => {
		try {
			globalThis.sessionStorage?.setItem(STORAGE_KEY, JSON.stringify(state));
		} catch {
			// Not persisted; the in-memory state still works.
		}
	};

	const view = (module: MockModule): ModuleSettingsView => {
		const overrides = state.modules[module.summary.id] ?? defaultOverrides();
		return {
			...module.summary,
			enabled: overrides.enabled,
			limits: overrides.limits,
			module_limits: module.limits,
			http: overrides.http,
			options: module.options.map((o) => {
				const stored = overrides.options[o.key];
				return (stored === undefined ? o : { ...o, value: stored }) as ModuleOptionSetting;
			})
		};
	};

	const find = (id: string) => MODULES.find((m) => m.summary.id === id);

	return {
		getSettings: (): Settings => structuredClone(state.settings),

		/** @throws Invalid */
		patchSettings(patch: JsonObject): Settings {
			const next = structuredClone(state.settings);
			mergePatch(next, patch, defaultSettings());
			validateSettings(next);
			state.settings = next;
			save();
			return structuredClone(next);
		},

		previewRename,

		listModules: (): ModuleSummary[] =>
			MODULES.map((m) => ({ ...m.summary, option_count: m.options.length })),

		getModule(id: string): ModuleSettingsView | null {
			const module = find(id);
			return module ? view(module) : null;
		},

		/** `null` when there is no such module. @throws Invalid */
		patchModule(id: string, patch: JsonObject): ModuleSettingsView | null {
			const module = find(id);
			if (!module) return null;
			const next = structuredClone(state.modules[id] ?? defaultOverrides());
			const { options, ...rest } = patch;
			const { options: stored, ...current } = next;
			mergePatch(current, rest, defaultOverrides());
			if (options !== undefined && options !== null) {
				if (!isObject(options)) throw new Invalid('options', 'expected a JSON object');
				for (const [key, value] of Object.entries(options)) {
					const field = `options.${key}`;
					const option = module.options.find((o) => o.key === key);
					if (!option) throw new Invalid(field, `unknown setting ${field}`);
					if (value === null) {
						delete stored[key];
						continue;
					}
					const error = checkOption(option, value);
					if (error) throw new Invalid(field, error);
					stored[key] = value;
				}
			}
			state.modules[id] = { ...(current as Omit<Overrides, 'options'>), options: stored };
			save();
			return view(module);
		}
	};
}
