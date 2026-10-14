// Whether a library series' chapters are in its folder (`ChapterInfo.on_disk`).

/** Seen, but its file or folder isn't in the series' folder; unknown (`null`) doesn't count. */
export function missingFromDisk(chapter: {
	downloaded: boolean;
	on_disk?: boolean | null;
}): boolean {
	return chapter.downloaded && chapter.on_disk === false;
}
