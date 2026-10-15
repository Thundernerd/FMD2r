import type { Channel, NotificationType, OutputFormat } from '#lib/api/types.ts';
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
const swatches = (
	path: string,
	label: string,
	choices: Choice[],
	help?: string,
	extra: Omit<Extract<Control, { kind: 'swatches' }>, 'kind' | 'choices'> = {}
): Field => ({ path, label, help, control: { kind: 'swatches', choices, ...extra } });
const themes = (path: string, label: string, choices: Choice[], accentPath: string): Field => ({
	path,
	label,
	control: { kind: 'themes', choices, accentPath }
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

/** A way to save chapters (`output.format`), with what it suits, as the setup wizard shows it. */
export interface OutputFormatChoice extends Choice {
	value: OutputFormat;
	description: string;
}

/** The choices of `output.format`, for Settings and the setup wizard alike. */
export const OUTPUT_FORMATS: OutputFormatChoice[] = [
	{
		value: 'folder',
		label: 'Folder of images',
		description: 'Each chapter as a folder of image files, to browse in a file manager.'
	},
	{
		value: 'zip',
		label: 'ZIP',
		description: 'Each chapter as one ZIP archive of its images, to store or share.'
	},
	{
		value: 'cbz',
		label: 'CBZ',
		description: 'For comic readers such as Komga, Kavita or a tablet app.'
	},
	{ value: 'pdf', label: 'PDF', description: 'For anything that opens PDFs.' },
	{ value: 'epub', label: 'EPUB', description: 'For e-readers.' }
];

/** Settings the Settings page edits in a section of their own rather than as a field. */
export const OWN_SECTION_PATHS = ['general.selected_websites'];

/**
 * Settings a section edits with a control of its own besides its fields, by section id. Their
 * errors are keyed by paths under these, e.g. `saveto.destinations.1.name`.
 */
export const SECTION_EXTRA_PATHS: Record<string, string[]> = {
	saveto: ['saveto.destinations']
};

/** Settings the setup wizard keeps (whether it was finished, where to resume), with no control. */
export const SETUP_PATHS = ['general.setup_completed', 'general.setup_step'];

/** Settings with no control of their own: the server keeps them in step with others. */
export const DERIVED_PATHS = [
	// The default destination's folder, for API clients that predate destinations.
	'saveto.default_dir'
];

/** A notification channel (`Channel`), as the Notifications section titles it. */
export interface NotificationChannel {
	id: Channel;
	name: string;
}

export const NOTIFICATION_CHANNELS: NotificationChannel[] = [
	{ id: 'ntfy', name: 'ntfy' },
	{ id: 'discord', name: 'Discord' },
	{ id: 'webhook', name: 'Webhook' }
];

/** The kinds of event a channel can send (`fmd_core::notifications::NotificationType::label`). */
const NOTIFICATION_TYPES: { value: NotificationType; label: string }[] = [
	{ value: 'new_chapters', label: 'New chapters' },
	{ value: 'missing_chapters', label: 'Missing chapters' },
	{ value: 'missing_check', label: 'Missing-chapter check found nothing' },
	{ value: 'completed', label: 'Completed series removed' },
	{ value: 'download_failed', label: 'Download failed' },
	{ value: 'favorites_check_failed', label: 'Library check failed' },
	{ value: 'module_update', label: 'Module updates' },
	{ value: 'downloads_moved', label: 'Downloads moved' },
	{ value: 'comic_server', label: 'Comic server rescan' }
];

/** Where a channel sends to, and its credentials. */
function channelFields(id: Channel): Field[] {
	switch (id) {
		case 'ntfy':
			return [
				text('notifications.ntfy.topic_url', 'Topic URL', 'e.g. https://ntfy.sh/my-topic'),
				secret('notifications.ntfy.token', 'Access token', 'Optional.')
			];
		case 'discord':
			return [
				secret(
					'notifications.discord.webhook_url',
					'Webhook URL',
					'From the channel’s Integrations settings. Stored encrypted: it holds a token.'
				)
			];
		case 'webhook':
			return [
				text(
					'notifications.webhook.url',
					'URL',
					'Gets a JSON POST with kind, severity, title, body and created_at.'
				),
				secret(
					'notifications.webhook.authorization',
					'Authorization header',
					'Optional, e.g. Bearer <token>.'
				)
			];
	}
}

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
		id: 'appearance',
		title: 'Appearance',
		fields: [
			themes(
				'appearance.theme',
				'Theme',
				[
					{ value: 'default', label: 'Default' },
					{ value: 'high-contrast', label: 'High contrast' },
					{ value: 'warm', label: 'Warm' },
					{ value: 'compact', label: 'Compact' }
				],
				'appearance.accent'
			),
			select(
				'appearance.mode',
				'Light or dark',
				[
					{ value: 'system', label: 'Same as the device' },
					{ value: 'light', label: 'Light' },
					{ value: 'dark', label: 'Dark' }
				],
				'Applies on every device.'
			),
			select('appearance.text_size', 'Text size', [
				{ value: 'small', label: 'Small' },
				{ value: 'normal', label: 'Normal' },
				{ value: 'large', label: 'Large' },
				{ value: 'larger', label: 'Larger' }
			]),
			swatches(
				'appearance.accent',
				'Accent colour',
				[
					{ value: 'teal', label: 'Teal' },
					{ value: 'blue', label: 'Blue' },
					{ value: 'green', label: 'Green' },
					{ value: 'purple', label: 'Purple' },
					{ value: 'orange', label: 'Orange' },
					{ value: 'red', label: 'Red' }
				],
				undefined,
				{
					lockedWhen: {
						path: 'appearance.theme',
						value: 'high-contrast',
						note: 'High contrast keeps its own accent, so links and buttons stay at 7:1.'
					}
				}
			),
			select(
				'appearance.library_title',
				'Library titles',
				[
					{ value: 'website', label: 'As the website names it' },
					{ value: 'english', label: 'English' },
					{ value: 'romanized', label: 'Romaji' },
					{ value: 'native', label: 'Native' },
					{ value: 'mangabaka', label: 'MangaBaka’s main title' }
				],
				'Needs the MangaBaka database. Series without a match keep the website’s title.'
			)
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
			checkbox('connections.schedule_enabled', 'Only download during set hours'),
			text('connections.schedule_start', 'From', 'HH:MM, in the server’s local time.', {
				placeholder: '23:00'
			}),
			text(
				'connections.schedule_end',
				'Until',
				'HH:MM. Before “From”, the hours cross midnight; equal to it, downloads run all day.',
				{ placeholder: '07:00' }
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
			select('output.format', 'Save chapters as', OUTPUT_FORMATS),
			number('output.pdf_quality', 'PDF image quality', 5, 100),
			select(
				'output.comic_metadata',
				'Metadata in CBZ files',
				[
					{ value: 'none', label: 'None' },
					{ value: 'comicinfo', label: 'ComicInfo.xml' },
					{ value: 'metroninfo', label: 'MetronInfo.xml' },
					{ value: 'both', label: 'Both' }
				],
				'Read by comic readers such as Komga and Kavita. Filled from the MangaBaka database when the series has a match.'
			)
		]
	},
	{
		id: 'comic_server',
		title: 'Comic server',
		fields: [
			select(
				'comic_server.kind',
				'Server',
				[
					{ value: 'none', label: 'None' },
					{ value: 'komga', label: 'Komga' },
					{ value: 'kavita', label: 'Kavita' }
				],
				'Asked to scan its libraries a minute after downloads finish, so new chapters show up right away.'
			),
			text('comic_server.url', 'URL', 'Where the server is reached from FMD2r.', {
				placeholder: 'http://nas:25600'
			}),
			secret('comic_server.api_key', 'API key', 'Made in the server’s account settings.'),
			text(
				'comic_server.library_id',
				'Komga library ID',
				'Komga only. Leave empty to scan every library the API key can see.',
				{ nullable: true }
			)
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
		id: 'notifications',
		title: 'Notifications',
		fields: NOTIFICATION_CHANNELS.flatMap(({ id, name }) => [
			checkbox(`notifications.${id}.enabled`, `Send to ${name}`),
			...channelFields(id),
			{
				path: `notifications.${id}.types`,
				label: 'Send these',
				control: { kind: 'checklist', choices: NOTIFICATION_TYPES }
			}
		])
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
			text(
				'server.public_url',
				'Address of FMD2r',
				'Where you open FMD2r, e.g. https://fmd.example.org. Notifications link to it.'
			),
			secret('server.auth_token', 'Password', 'Clear it to disable authentication.'),
			checkbox(
				'server.external_auth',
				'Authentication is handled in front of FMD2r',
				'e.g. a reverse proxy that asks for a login. Hides the warning about having no password.'
			),
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
