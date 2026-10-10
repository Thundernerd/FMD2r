// @vitest-environment jsdom
import { render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Api } from '#lib/api/client.ts';
import type { AccountInfo } from '#lib/api/types.ts';
import { summary } from '../modules.fixture.ts';
import AccountsPanel from './AccountsPanel.svelte';

const account = (module: string, name: string): AccountInfo => ({
	module,
	name,
	enabled: false,
	username: '',
	has_password: false,
	status: 'unknown'
});

const MODULES = [
	summary('a1', 'mangadex', 'https://mangadex.org', 'English'),
	summary('b2', 'Batoto', 'https://bato.to', 'English'),
	summary('c3', 'Pixiv', 'https://www.pixiv.net', 'Raw'),
	summary('d4', 'Pixiv', 'https://pixiv.example', 'Raw'),
	summary('e5', 'Unrelated', 'https://unrelated.example', 'English')
];

function setup(accounts: AccountInfo[]) {
	const api = { listAccounts: vi.fn(() => Promise.resolve(accounts)) } as unknown as Api;
	render(AccountsPanel, { api, modules: MODULES });
}

/** The category headings and the account names under each, in the order they are listed. */
function listed() {
	return screen.getAllByRole('list').map((list) => ({
		category: list.getAttribute('aria-label'),
		names: within(list)
			.getAllByRole('checkbox')
			.map((box) => box.closest('label')?.textContent?.trim())
	}));
}

describe('AccountsPanel', () => {
	it('lists accounts by name under their categories, like the website modules', async () => {
		// `GET /api/accounts` lists accounts by module ID.
		setup([
			account('a1', 'mangadex'),
			account('b2', 'Batoto'),
			account('c3', 'Pixiv'),
			account('d4', 'Pixiv'),
			account('z9', 'Loading')
		]);
		await screen.findAllByRole('checkbox');
		expect(screen.getAllByRole('heading').map((h) => h.textContent)).toEqual([
			'English',
			'Other',
			'Raw'
		]);
		expect(listed()).toEqual([
			{ category: 'English', names: ['Batoto', 'mangadex'] },
			// An account whose module summary isn't loaded yet is still listed.
			{ category: 'Other', names: ['Loading'] },
			{ category: 'Raw', names: ['Pixiv (pixiv.example)', 'Pixiv (www.pixiv.net)'] }
		]);
	});
});
