// @vitest-environment jsdom
import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import type { MangaBakaStatus } from '#lib/api/types.ts';
import MangaBakaHint from './MangaBakaHint.svelte';

const NONE: MangaBakaStatus = {
	available: true,
	downloaded: false,
	built_at: null,
	bytes: null,
	running: false,
	progress: null,
	next_refresh: null
};

describe('MangaBakaHint', () => {
	beforeEach(() => localStorage.clear());

	it('points to the setting until a database is downloaded', () => {
		render(MangaBakaHint, { status: NONE });

		const hint = screen.getByRole('note');
		expect(hint.textContent?.replace(/\s+/g, ' ')).toContain(
			'Get formats, publication status and descriptions for list titles from MangaBaka (~390 MB download).'
		);
		const link = screen.getByRole('link', { name: /MangaBaka database/ });
		expect(link.getAttribute('href')).toBe('/settings#section-metadata');
	});

	it('is not shown once a database is downloaded, or where none can be', () => {
		const { unmount } = render(MangaBakaHint, { status: { ...NONE, downloaded: true } });
		expect(screen.queryByRole('note')).toBeNull();
		unmount();
		render(MangaBakaHint, { status: { ...NONE, available: false } });
		expect(screen.queryByRole('note')).toBeNull();
	});

	it('stays dismissed', async () => {
		const { unmount } = render(MangaBakaHint, { status: NONE });
		await fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
		expect(screen.queryByRole('note')).toBeNull();
		unmount();

		render(MangaBakaHint, { status: NONE });
		expect(screen.queryByRole('note')).toBeNull();
	});
});
