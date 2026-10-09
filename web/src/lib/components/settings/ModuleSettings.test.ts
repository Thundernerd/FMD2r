// @vitest-environment jsdom
import { render, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { ModuleSummary } from '#lib/api/types.ts';
import ModuleSettings from './ModuleSettings.svelte';

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
// "HachiRaw" once Init has run.
const HACHIRAW = [
	module('1d09f3bea8f148fa9e9215fc578fedcd', 'HachiRaw', 'https://manga1001.win'),
	module('1d09f3bea8f148fa9e9215fc578fedcd', 'HachiRaw', 'https://hachiraw.win'),
	module('other', 'Other', 'https://other.example')
];

describe('ModuleSettings', () => {
	it('lists modules sharing an ID, told apart by host', () => {
		render(ModuleSettings, {
			modules: HACHIRAW,
			selected: null,
			view: null,
			draft: null,
			loading: false,
			onselect: () => {}
		});
		const picks = within(screen.getByRole('list', { name: 'Modules' })).getAllByRole('button');
		expect(picks.map((b) => b.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
			'HachiRaw manga1001.win',
			'HachiRaw hachiraw.win',
			'Other'
		]);
	});
});
