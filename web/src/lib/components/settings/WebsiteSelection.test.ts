// @vitest-environment jsdom
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { Draft } from '#lib/settings/draft.svelte.ts';
import { HACHIRAW, HACHIRAW_ID, summary } from '../modules.fixture.ts';
import WebsiteSelection from './WebsiteSelection.svelte';

const MODULES = [
	...HACHIRAW,
	summary('mangadex', 'MangaDex', 'https://mangadex.org', 'English'),
	summary('webtoons', 'Webtoons', 'https://www.webtoons.com', 'English')
];

/** FMD2's website selection: a checkbox per module, grouped by category (`tsWebsiteSelection`,
 * mangadownloader/forms/frmMain.pas:3315-3317). */
function renderSelection(selected: string[]) {
	const draft = new Draft<object>({ general: { selected_websites: selected } });
	render(WebsiteSelection, { modules: MODULES, draft });
	return draft;
}

const selection = (draft: Draft<object>) => draft.get('general.selected_websites');
const box = (name: string) => screen.getByRole('checkbox', { name }) as HTMLInputElement;

describe('WebsiteSelection', () => {
	it('checks the selected websites and counts them', () => {
		renderSelection(['mangadex', 'gone']);
		expect(box('MangaDex').checked).toBe(true);
		expect(box('Webtoons').checked).toBe(false);
		expect(screen.getByText('1 of 4 websites selected')).toBeTruthy();
	});

	it('updates the draft from a checkbox', async () => {
		const draft = renderSelection(['mangadex', 'gone']);
		await fireEvent.click(box('Webtoons'));
		expect(selection(draft)).toEqual(['mangadex', 'gone', 'webtoons']);
		await fireEvent.click(box('MangaDex'));
		// A module that is not loaded stays selected for when it comes back.
		expect(selection(draft)).toEqual(['gone', 'webtoons']);
	});

	it('selects all and none of the websites the search shows', async () => {
		const draft = renderSelection(['mangadex']);
		await fireEvent.input(screen.getByRole('searchbox', { name: 'Search websites' }), {
			target: { value: 'raw' }
		});
		expect(screen.queryByRole('checkbox', { name: 'MangaDex' })).toBeNull();

		await fireEvent.click(screen.getByRole('button', { name: 'Select all' }));
		expect(selection(draft)).toEqual(['mangadex', HACHIRAW_ID, 'other']);

		await fireEvent.click(screen.getByRole('button', { name: 'Select none' }));
		expect(selection(draft)).toEqual(['mangadex']);
	});
});
