/** A download speed for display, e.g. `1.8 MB/s`. */
export function formatRate(bytesPerSec: number): string {
	if (bytesPerSec >= 1_000_000) return `${(bytesPerSec / 1_000_000).toFixed(1)} MB/s`;
	if (bytesPerSec >= 1_000) return `${Math.round(bytesPerSec / 1_000)} kB/s`;
	return `${Math.round(bytesPerSec)} B/s`;
}

/** Whole percent of `done` out of `total`; 0 when the total is unknown. */
export function percent(done: number, total: number): number {
	return total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
}
