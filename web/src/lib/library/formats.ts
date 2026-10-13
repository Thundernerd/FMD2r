/** MangaBaka's formats, as fmd-server names them (`FORMATS` in fmd-core), with their labels. */
export const FORMATS = [
	['manga', 'Manga'],
	['manhwa', 'Manhwa'],
	['manhua', 'Manhua'],
	['oel', 'OEL'],
	['other', 'Other']
] as const;

/** `format`'s label; the value itself for one this build doesn't know. */
export function formatLabel(format: string): string {
	return FORMATS.find(([value]) => value === format)?.[1] ?? format;
}
