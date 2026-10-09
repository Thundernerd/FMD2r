// @vitest-environment jsdom
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { ModuleSummary } from '#lib/api/types.ts';
import { HACHIRAW, HACHIRAW_ID, summary } from '../modules.fixture.ts';
import ModuleSettings from './ModuleSettings.svelte';

const picks = () => within(screen.getByRole('list', { name: 'Modules' })).getAllByRole('button');

const text = (el: HTMLElement) => el.textContent?.replace(/\s+/g, ' ').trim();

/** Modules in ID order, as `GET /api/modules` returns them. */
const BY_ID: ModuleSummary[] = [
	{ ...summary('3a', 'mangaDex', 'https://mangadex.org'), category: 'English' },
	{ ...summary('7c', 'Bato.to', 'https://bato.to'), category: 'English' },
	{ ...summary('9f', 'Rawkuma', 'https://rawkuma.com'), category: 'Raw' },
	{ ...summary('b1', 'ComicK', 'https://comick.io'), category: 'English' },
	{ ...summary('c4', 'Lone', 'https://lone.example'), category: '' }
];

const props = { selected: null, view: null, draft: null, loading: false };

describe('ModuleSettings', () => {
	it('lists modules sharing a name by host, told apart by it', () => {
		render(ModuleSettings, { ...props, modules: HACHIRAW, onselect: () => {} });
		expect(picks().map(text)).toEqual(['HachiRaw hachiraw.win', 'HachiRaw manga1001.win', 'Other']);
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

	it('lists modules by name under their category headings', () => {
		render(ModuleSettings, { ...props, modules: BY_ID, onselect: () => {} });
		const groups = within(screen.getByRole('list', { name: 'Modules' }))
			.getAllByRole('heading')
			.map((h) => text(h));
		expect(groups).toEqual(['English', 'Other', 'Raw']);
		const english = within(screen.getByRole('list', { name: 'English' })).getAllByRole('button');
		expect(english.map(text)).toEqual(['Bato.to', 'ComicK', 'mangaDex']);
		expect(within(screen.getByRole('list', { name: 'Other' })).getByRole('button')).toHaveProperty(
			'textContent',
			expect.stringContaining('Lone')
		);
	});

	it('counts the modules, and the matches while searching', async () => {
		render(ModuleSettings, { ...props, modules: BY_ID, onselect: () => {} });
		expect(screen.getByText('5 modules')).toBeTruthy();
		await fireEvent.input(screen.getByRole('searchbox', { name: 'Search modules' }), {
			target: { value: 'comi' }
		});
		expect(screen.getByText('1 of 5')).toBeTruthy();
	});

	it('finds the modules of a category, and a module by its host', async () => {
		render(ModuleSettings, { ...props, modules: BY_ID, onselect: () => {} });
		const search = screen.getByRole('searchbox', { name: 'Search modules' });
		await fireEvent.input(search, { target: { value: 'english' } });
		expect(picks().map(text)).toEqual(['Bato.to', 'ComicK', 'mangaDex']);
		await fireEvent.input(search, { target: { value: 'rawkuma.com' } });
		expect(picks().map(text)).toEqual(['Rawkuma']);
	});

	it('marks the modules with custom settings', () => {
		const modules = BY_ID.map((m) => ({ ...m, customized: m.name === 'ComicK' }));
		render(ModuleSettings, { ...props, modules, onselect: () => {} });
		const marked = picks().filter((b) => within(b).queryByText('Custom'));
		expect(marked.map(text)).toEqual(['ComicK Custom']);
	});

	it('moves through the list with the arrow keys, across categories', async () => {
		render(ModuleSettings, { ...props, modules: BY_ID, onselect: () => {} });
		const search = screen.getByRole('searchbox', { name: 'Search modules' });
		search.focus();
		await fireEvent.keyDown(search, { key: 'ArrowDown' });
		expect(text(document.activeElement as HTMLElement)).toBe('Bato.to');
		for (let i = 0; i < 3; i++) {
			await fireEvent.keyDown(document.activeElement as HTMLElement, { key: 'ArrowDown' });
		}
		expect(text(document.activeElement as HTMLElement)).toBe('Lone');
		await fireEvent.keyDown(document.activeElement as HTMLElement, { key: 'ArrowUp' });
		expect(text(document.activeElement as HTMLElement)).toBe('mangaDex');
	});
});
