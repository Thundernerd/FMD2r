// @vitest-environment jsdom
import { render, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { ModuleSummary } from '#lib/api/types.ts';
import WebsitePicker from './WebsitePicker.svelte';

const module = (id: string, name: string, root_url: string): ModuleSummary => ({
	id,
	name,
	root_url,
	category: 'Raw',
	option_count: 0,
	capabilities: { update_list: true, info: true, download: true, account: false },
	list_size: 0,
	list_updated: null,
	list_job_running: false
});

// Upstream lua/modules/Manga1001.lua:18-19 registers two websites under one ID, both named
// "HachiRaw" once Init has run, both in the "Raw" group.
const HACHIRAW = [
	module('1d09f3bea8f148fa9e9215fc578fedcd', 'HachiRaw', 'https://manga1001.win'),
	module('1d09f3bea8f148fa9e9215fc578fedcd', 'HachiRaw', 'https://hachiraw.win'),
	module('other', 'Other', 'https://other.example')
];

describe('WebsitePicker', () => {
	it('lists modules sharing an ID, told apart by host', () => {
		render(WebsitePicker, { modules: HACHIRAW, selected: '' });
		const group = screen.getByRole('group', { name: 'Raw' });
		const options = within(group).getAllByRole('option');
		expect(options.map((o) => [o.getAttribute('value'), o.textContent?.trim()])).toEqual([
			['1d09f3bea8f148fa9e9215fc578fedcd', 'HachiRaw (hachiraw.win)'],
			['1d09f3bea8f148fa9e9215fc578fedcd', 'HachiRaw (manga1001.win)'],
			['other', 'Other']
		]);
	});
});
