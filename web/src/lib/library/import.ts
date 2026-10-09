import type { ImportReport, SourceReport } from '#lib/api/types.ts';

/** The report's sources, as `fmd2r import` prints them. */
export const SOURCES: { key: SourceKey; label: string }[] = [
	{ key: 'tasks', label: 'downloads.db → tasks' },
	{ key: 'favorites', label: 'favorites.db → favorites' },
	{ key: 'downloaded_chapters', label: 'downloadedchapters.db → seen chapters' },
	{ key: 'module_settings', label: 'modules.json → module settings' },
	{ key: 'accounts', label: 'modules.json → accounts' },
	{ key: 'settings', label: 'settings.json → settings' }
];

/** The fields of an {@link ImportReport} that report a source. */
type SourceKey = {
	[K in keyof ImportReport]: ImportReport[K] extends SourceReport ? K : never;
}[keyof ImportReport];

/** How many of a source's items the store already had. */
export const alreadyExisting = (source: SourceReport): number =>
	source.skipped.filter((s) => s.reason.kind === 'already_exists').length;

/** The skipped items of a source that were invalid, with why. */
export const invalid = (source: SourceReport): { item: string; why: string }[] =>
	source.skipped.flatMap((s) =>
		s.reason.kind === 'invalid' ? [{ item: s.item, why: s.reason.detail }] : []
	);

/** The non-blank lines of the path maps field, each `FROM=TO`. */
export const pathMaps = (text: string): string[] =>
	text
		.split('\n')
		.map((l) => l.trim())
		.filter((l) => l !== '');
