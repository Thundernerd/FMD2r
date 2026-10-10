// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { api } from '#lib/app.ts';
import Discover from './+page.svelte';

// The mock backend with a MangaBaka database downloaded: its titles carry formats and
// publication statuses, `unknown` for those without a match.
vi.mock('#lib/app.ts', async () => {
	const { createApi } = await import('#lib/api/client.ts');
	const { createMockBackend } = await import('#lib/api/mock.ts');
	const fetch = createMockBackend({ mangabaka: true }).fetch;
	return { api: createApi({ baseUrl: 'http://fmd2r.test', fetch }), events: {} };
});

/** The result count the page shows, e.g. "7 titles". */
const shown = () => screen.getByRole('status').textContent?.replace(/\s+/g, ' ').trim();

/** The count an option's label gives, e.g. 7 for "Manhwa (7)". */
const countOf = (select: HTMLElement, label: string): number => {
	const option = optionLabels(select).find((l) => l?.startsWith(`${label} (`));
	return Number(/\((\d+)\)$/.exec(option ?? '')?.[1] ?? NaN);
};

const optionLabels = (select: HTMLElement) =>
	within(select)
		.getAllByRole('option')
		.map((o) => o.textContent?.trim());

describe('Discover with the MangaBaka database', () => {
	it('groups Format and Publication as metadata filters', async () => {
		await api.patchSettings({ general: { selected_websites: ['webtoons'] } });
		render(Discover);

		const metadata = await screen.findByRole('group', { name: 'Metadata filters' });
		expect(within(metadata).getByText('From MangaBaka.')).toBeTruthy();
		expect(await within(metadata).findByRole('combobox', { name: 'Format' })).toBeTruthy();
		expect(within(metadata).getByRole('combobox', { name: 'Publication' })).toBeTruthy();
		expect(within(metadata).queryByRole('link')).toBeNull();
		const website = screen.getByRole('group', { name: 'Website filters' });
		expect(within(website).queryByRole('combobox', { name: 'Format' })).toBeNull();
		expect(website.compareDocumentPosition(metadata)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
	});

	it('filters by format', async () => {
		await api.patchSettings({ general: { selected_websites: ['webtoons'] } });
		render(Discover);

		const format = await screen.findByRole('combobox', { name: 'Format' });
		await waitFor(() => expect(countOf(format, 'Manhwa')).toBeGreaterThan(0));
		const manhwa = countOf(format, 'Manhwa');
		expect(optionLabels(format)[0]).toBe('Any');
		expect(countOf(format, 'Unknown')).toBeGreaterThan(0);
		expect(screen.queryByRole('note')).toBeNull();

		await fireEvent.change(format, { target: { value: 'manhwa' } });

		await waitFor(() => expect(shown()).toBe(`${manhwa} titles`));
		const cards = screen.getAllByRole('link').filter((a) => a.classList.contains('card'));
		expect(cards.length).toBe(manhwa);
		for (const card of cards) expect(card.textContent).toContain('Manhwa');
	});

	it('filters by publication status', async () => {
		await api.patchSettings({ general: { selected_websites: ['webtoons'] } });
		render(Discover);

		const publication = await screen.findByRole('combobox', { name: 'Publication' });
		await waitFor(() => expect(countOf(publication, 'Hiatus')).toBeGreaterThan(0));
		const hiatus = countOf(publication, 'Hiatus');
		expect(countOf(publication, 'Unknown')).toBeGreaterThan(0);

		await fireEvent.change(publication, { target: { value: 'hiatus' } });

		await waitFor(() => expect(shown()).toBe(`${hiatus} titles`));
		const cards = screen.getAllByRole('link').filter((a) => a.classList.contains('card'));
		for (const card of cards) expect(card.textContent).toContain('Hiatus');
	});
});
