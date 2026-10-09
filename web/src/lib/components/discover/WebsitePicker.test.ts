// @vitest-environment jsdom
import { render, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { HACHIRAW, HACHIRAW_ID } from '../modules.fixture.ts';
import WebsitePicker from './WebsitePicker.svelte';

describe('WebsitePicker', () => {
	it('lists modules sharing an ID, told apart by host', () => {
		render(WebsitePicker, {
			modules: HACHIRAW,
			websites: [HACHIRAW_ID, 'other'],
			selected: ''
		});
		const options = within(screen.getByRole('group', { name: 'Raw' })).getAllByRole('option');
		expect(options.map((o) => [o.getAttribute('value'), o.textContent?.trim()])).toEqual([
			[HACHIRAW_ID, 'HachiRaw (hachiraw.win)'],
			[HACHIRAW_ID, 'HachiRaw (manga1001.win)'],
			['other', 'Other']
		]);
	});

	// Only the checked websites appear in FMD2's website dropdown
	// (mangadownloader/forms/frmMain.pas:6464-6487).
	it('lists only the selected websites', () => {
		render(WebsitePicker, { modules: HACHIRAW, websites: ['other', 'gone'], selected: '' });
		const options = screen.getAllByRole('option');
		expect(options.map((o) => o.textContent?.trim())).toEqual(['All websites', 'Other']);
	});
});
