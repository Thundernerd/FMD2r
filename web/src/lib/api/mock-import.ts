import type { ImportReport, SourceReport } from './types';

/** The Windows save-to folder of the mock userdata's second task. */
const WINDOWS_SAVE_TO = 'C:\\Manga\\Guts';

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
 * chapters row, a module with an account, and two settings, one of them invalid.
 */
export function mockImportReport(
	dryRun: boolean,
	mapPaths: string[],
	favoriteExists: boolean
): ImportReport {
	const mapped = mapPaths.some((m) => {
		const from = m.split('=')[0] ?? '';
		return from !== '' && WINDOWS_SAVE_TO.toLowerCase().startsWith(from.toLowerCase());
	});
	return {
		dry_run: dryRun,
		tasks: source(2),
		favorites: source(1, favoriteExists ? 1 : 0),
		downloaded_chapters: source(1),
		module_settings: source(1),
		accounts: source(1),
		settings: {
			found: true,
			imported: 1,
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
