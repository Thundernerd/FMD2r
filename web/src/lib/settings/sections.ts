import type { Choice, Control, Field } from './fields.ts';

/** A group of settings: one entry in the table of contents. */
export interface Section {
	id: string;
	title: string;
	fields: Field[];
}

const checkbox = (path: string, label: string, help?: string): Field => ({
	path,
	label,
	help,
	control: { kind: 'checkbox' }
});
const text = (
	path: string,
	label: string,
	help?: string,
	extra: Omit<Extract<Control, { kind: 'text' }>, 'kind'> = {}
): Field => ({ path, label, help, control: { kind: 'text', ...extra } });
/** A password or token: write-only, see `Control`. */
const secret = (path: string, label: string, help?: string): Field => ({
	path,
	label,
	help,
	control: { kind: 'secret' }
});
const number = (path: string, label: string, min: number, max: number, help?: string): Field => ({
	path,
	label,
	help,
	control: { kind: 'number', min, max }
});
const select = (path: string, label: string, choices: Choice[], help?: string): Field => ({
	path,
	label,
	help,
	control: { kind: 'select', choices }
});

/** No upper bound beyond what the server's `u32` holds. */
const U32_MAX = 4_294_967_295;
const RENAME_TOKENS =
	'Tokens: %MANGA% %CHAPTER% %NUMBERING% %WEBSITE% %AUTHOR% %ARTIST% %FILENAME%.';

/** Settings the Settings page edits in a section of their own rather than as a field. */
export const OWN_SECTION_PATHS = ['general.selected_websites'];

/**
 * Settings a section edits with a control of its own besides its fields, by section id. Their
 * errors are keyed by paths under these, e.g. `saveto.destinations.1.name`.
 */
export const SECTION_EXTRA_PATHS: Record<string, string[]> = {
	saveto: ['saveto.destinations']
};

/** Settings with no control of their own: the server keeps them in step with others. */
export const DERIVED_PATHS = [
	// The default destination's folder, for API clients that predate destinations.
	'saveto.default_dir'
];

/**
 * Every application setting (the T18 model), one section per settings group. Ranges are the
 * server's: FMD2's spin edit bounds, or FMD2r's own for settings FMD2 does not have.
 */
export const SETTINGS_SECTIONS: Section[] = [
	{
		id: 'general',
		title: 'General',
		fields: [
			text('general.language', 'Language', 'UI language code, e.g. en.'),
			checkbox('general.add_as_stopped', 'Add new downloads stopped'),
			checkbox('general.load_covers', 'Load manga covers'),
			text(
				'general.data_dir',
				'Data folder',
				'For the list databases, relative to the app data directory.'
			),
			text('general.lua_dir', 'Lua folder', 'Holds the website modules.')
		]
	},
	{
		id: 'connections',
		title: 'Connections',
		fields: [
			number('connections.max_parallel_tasks', 'Parallel downloads', 1, 64, 'Over all websites.'),
			number('connections.threads_per_task', 'Threads per download', 1, 256),
			number('connections.retry_count', 'Retries per request', -1, 5, '-1 retries forever.'),
			number(
				'connections.auto_retry_failed_tasks',
				'Restart failed downloads',
				0,
				100,
				'Times a failed download is restarted.'
			),
			checkbox('connections.always_start_from_failed_chapters', 'Restart from the failed chapters'),
			number('connections.max_favorite_threads', 'Favorite check threads', 1, 32),
			number('connections.max_update_list_threads', 'List update threads', 1, 32),
			number('connections.timeout_secs', 'Connection timeout (seconds)', 1, 300),
			text('connections.user_agent', 'User agent', 'Leave empty for the default.'),
			text(
				'connections.flaresolverr_url',
				'FlareSolverr URL',
				'Solves Cloudflare challenges, e.g. http://flaresolverr:8191. Takes effect after a restart.'
			),
			checkbox('connections.proxy.enabled', 'Use a proxy'),
			select('connections.proxy.type', 'Proxy type', [
				{ value: 'http', label: 'HTTP' },
				{ value: 'socks4', label: 'SOCKS4' },
				{ value: 'socks5', label: 'SOCKS5' }
			]),
			text('connections.proxy.host', 'Proxy host'),
			{
				path: 'connections.proxy.port',
				label: 'Proxy port',
				control: { kind: 'number', min: 1, max: 65535, nullable: true }
			},
			text('connections.proxy.username', 'Proxy username'),
			secret('connections.proxy.password', 'Proxy password')
		]
	},
	{
		id: 'saveto',
		title: 'Save to',
		fields: [
			checkbox('saveto.generate_manga_folder', 'Create a folder per manga'),
			text('saveto.manga_rename', 'Manga folder name', RENAME_TOKENS),
			checkbox('saveto.generate_chapter_folder', 'Create a folder per chapter'),
			text('saveto.chapter_rename', 'Chapter name', RENAME_TOKENS),
			text('saveto.filename_rename', 'Page file name', RENAME_TOKENS),
			checkbox('saveto.remove_manga_name_from_chapter', 'Remove the manga name from chapter names'),
			checkbox('saveto.replace_unicode', 'Replace non-ASCII characters'),
			text('saveto.replace_unicode_with', 'Replace them with'),
			checkbox('saveto.convert_digit_volume', 'Pad volume numbers'),
			number('saveto.digit_volume_length', 'Volume digits', 1, 10),
			checkbox('saveto.convert_digit_chapter', 'Pad chapter numbers'),
			number('saveto.digit_chapter_length', 'Chapter digits', 1, 10),
			select(
				'saveto.illegal_chars',
				'Illegal characters',
				[
					{ value: 'posix', label: 'POSIX (replace / with _)' },
					{ value: 'windows', label: 'Windows (strip \\ / : * ? " < > | ;)' }
				],
				'Which characters are removed from file names.'
			)
		]
	},
	{
		id: 'output',
		title: 'Output',
		fields: [
			select('output.format', 'Save chapters as', [
				{ value: 'folder', label: 'Folder of images' },
				{ value: 'zip', label: 'ZIP' },
				{ value: 'cbz', label: 'CBZ' },
				{ value: 'pdf', label: 'PDF' },
				{ value: 'epub', label: 'EPUB' }
			]),
			number('output.pdf_quality', 'PDF image quality', 5, 100)
		]
	},
	{
		id: 'images',
		title: 'Images',
		fields: [
			checkbox('images.png_to_jpeg', 'Save PNG as JPEG'),
			select('images.webp_save_as', 'Save WebP as', [
				{ value: 'webp', label: 'WebP (keep)' },
				{ value: 'png', label: 'PNG' },
				{ value: 'jpeg', label: 'JPEG' }
			]),
			select('images.png_compression', 'PNG compression', [
				{ value: 'none', label: 'None' },
				{ value: 'fastest', label: 'Fastest' },
				{ value: 'default', label: 'Default' },
				{ value: 'maximum', label: 'Maximum' }
			]),
			number('images.jpeg_quality', 'JPEG quality', 1, 100),
			checkbox('images.imagemagick.enabled', 'Convert with ImageMagick'),
			text('images.imagemagick.save_as', 'ImageMagick format', 'e.g. JPEG.'),
			text(
				'images.imagemagick.compression',
				'ImageMagick compression',
				'A -compress type, e.g. None.'
			),
			number('images.imagemagick.quality', 'ImageMagick quality', 1, 100)
		]
	},
	{
		id: 'favorites',
		title: 'Favorites',
		fields: [
			checkbox('favorites.check_at_startup', 'Check for new chapters at startup'),
			checkbox('favorites.check_on_interval', 'Check for new chapters periodically'),
			number('favorites.check_interval_minutes', 'Check every (minutes)', 1, 1440),
			checkbox('favorites.auto_download', 'Download new chapters automatically'),
			checkbox('favorites.remove_completed', 'Remove completed manga')
		]
	},
	{
		id: 'update_lists',
		title: 'Manga lists',
		fields: [
			checkbox('update_lists.auto_update', 'Update lists automatically'),
			number('update_lists.interval_hours', 'Update every (hours)', 1, U32_MAX),
			checkbox('update_lists.no_manga_info', 'Skip manga info when updating'),
			checkbox('update_lists.remove_duplicate_local_data', 'Remove duplicate local data'),
			number('update_lists.new_manga_days', 'Entries count as new for (days)', 1, 365),
			text(
				'update_lists.db_url',
				'Ready-made lists URL',
				'From the FMD2-DB project by default. <website> is replaced by the module ID.'
			)
		]
	},
	{
		id: 'module_updater',
		title: 'Module updates',
		fields: [
			checkbox('module_updater.auto_update', 'Update modules automatically'),
			number('module_updater.interval_minutes', 'Check every (minutes)', 1, U32_MAX),
			text('module_updater.repo_owner', 'Repository owner'),
			text('module_updater.repo_name', 'Repository name'),
			text('module_updater.repo_ref', 'Branch'),
			text('module_updater.repo_path', 'Path in the repository'),
			secret('module_updater.github_token', 'GitHub token', 'Raises the API rate limit. Optional.'),
			checkbox('module_updater.keep_last_good', 'Keep the last working version of a broken module')
		]
	},
	{
		id: 'metadata',
		title: 'MangaBaka database',
		fields: [
			number(
				'metadata.mangabaka.refresh_days',
				'Update every (days)',
				0,
				365,
				'Once downloaded. 0 turns automatic updates off.'
			)
		]
	},
	{
		id: 'covers',
		title: 'Covers',
		fields: [
			number(
				'covers.revalidate_after_hours',
				'Recheck cached covers after (hours)',
				0,
				U32_MAX,
				'0 asks the website every time.'
			),
			number('covers.cache_size_mb', 'Cover cache size (MiB)', 1, U32_MAX)
		]
	},
	{
		id: 'logs',
		title: 'Logs',
		fields: [
			number(
				'logs.max_file_size_mb',
				'Log file size (MiB)',
				1,
				1024,
				'Takes effect after a restart.'
			),
			number(
				'logs.max_files',
				'Log files kept',
				1,
				100,
				'The oldest is deleted past this. Takes effect after a restart.'
			)
		]
	},
	{
		id: 'server',
		title: 'Server',
		fields: [
			text('server.bind', 'Listen address', 'Takes effect after a restart.'),
			secret('server.auth_token', 'Password', 'Clear it to disable authentication.'),
			number(
				'server.session_idle_days',
				'Log out after (days unused)',
				1,
				365,
				'Each visit restarts the count.'
			),
			number('server.session_lifetime_days', 'Log out after (days)', 1, 3650, 'However often used.')
		]
	},
	{
		id: 'xpath',
		title: 'XPath',
		fields: [
			select(
				'xpath.backend',
				'XPath engine',
				[
					{ value: 'fpc', label: 'FMD2 engine (internettools)' },
					{ value: 'native', label: 'Native (Rust)' }
				],
				'Which engine evaluates module XPath.'
			)
		]
	}
];
