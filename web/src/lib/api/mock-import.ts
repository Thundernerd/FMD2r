import type { ImportReport, SourceReport } from './types';

/** The Windows save-to folder of the mock userdata's second task. */
const WINDOWS_SAVE_TO = 'C:\\Manga\\Guts';
/** The mock userdata's download folder (`saveto/SaveTo`). */
const WINDOWS_DEFAULT_DIR = 'C:\\Manga';

/** `path` rewritten by the first path map that covers it, as fmd-import's `translate`. */
const translate = (mapPaths: string[], path: string): string | null => {
	for (const m of mapPaths) {
		const [from = '', to = ''] = m.split(/=(.*)/);
		if (from === '' || !path.toLowerCase().startsWith(from.toLowerCase())) continue;
		return (to + path.slice(from.length)).replaceAll('\\', '/');
	}
	return null;
};

const source = (imported: number, existing = 0): SourceReport => ({
	found: true,
	imported: imported - existing,
	skipped: Array.from({ length: existing }, (_, i) => ({
		item: `item ${i + 1}`,
		reason: { kind: 'already_exists' }
	}))
});

/**
 * What fmd-server reports for the mock's FMD2 userdata: two tasks (one saving to a Windows
 * folder), one favorite (One Piece, `existing` when it is in the library already), a downloaded
 * chapters row, a module with an account, and four settings, one of them invalid (the others are
 * {@link mockImportedSettings}).
 */
export function mockImportReport(
	dryRun: boolean,
	mapPaths: string[],
	favoriteExists: boolean
): ImportReport {
	const mapped = translate(mapPaths, WINDOWS_SAVE_TO) !== null;
	return {
		dry_run: dryRun,
		tasks: source(2),
		favorites: source(1, favoriteExists ? 1 : 0),
		downloaded_chapters: source(1),
		module_settings: source(1),
		accounts: source(1),
		settings: {
			found: true,
			imported: 3,
			skipped: [
				{
					item: 'connections/NumberOfTasks',
					reason: { kind: 'invalid', detail: 'connections.max_parallel_tasks: must be 1 to 8' }
				}
			]
		},
		unmapped: [{ source: 'settings.json', key: 'general/OneInstanceOnly', value: 'true' }],
		warnings: mapped
			? []
			: [`save-to path ${WINDOWS_SAVE_TO} is a Windows path that no path map covers`]
	};
}

/**
 * The settings the mock userdata's `settings.json` imports, as a merge patch: its download folder
 * (path-mapped), CBZ (`saveto/Compress` 2) and two websites (`general/MangaListSelect`).
 */
export const mockImportedSettings = (mapPaths: string[]) => ({
	saveto: { default_dir: translate(mapPaths, WINDOWS_DEFAULT_DIR) ?? WINDOWS_DEFAULT_DIR },
	output: { format: 'cbz' },
	general: { selected_websites: ['mangadex', 'comick'] }
});
