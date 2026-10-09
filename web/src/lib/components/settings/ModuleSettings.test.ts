// @vitest-environment jsdom
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { HACHIRAW, HACHIRAW_ID } from '../modules.fixture.ts';
import ModuleSettings from './ModuleSettings.svelte';

const picks = () => within(screen.getByRole('list', { name: 'Modules' })).getAllByRole('button');

const props = { selected: null, view: null, draft: null, loading: false };

describe('ModuleSettings', () => {
	it('lists modules sharing an ID, told apart by host', () => {
		render(ModuleSettings, { ...props, modules: HACHIRAW, onselect: () => {} });
		expect(picks().map((b) => b.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
			'HachiRaw manga1001.win',
			'HachiRaw hachiraw.win',
			'Other'
		]);
	});

	// FMD2 keys module settings by ID (baseunits/WebsiteModules.pas:545-700), so both entries
	// edit the same settings.
	it('selects the shared ID from either entry', async () => {
		const selected: string[] = [];
		render(ModuleSettings, {
			...props,
			modules: HACHIRAW,
			onselect: (id: string) => selected.push(id)
		});
		for (const pick of picks().slice(0, 2)) await fireEvent.click(pick);
		expect(selected).toEqual([HACHIRAW_ID, HACHIRAW_ID]);
	});
});
