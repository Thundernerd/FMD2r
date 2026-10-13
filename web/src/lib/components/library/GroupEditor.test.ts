// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { ApiError, type Api } from '#lib/api/client.ts';
import type { GroupPatch, GroupView, RuleView } from '#lib/api/types.ts';
import GroupEditor from './GroupEditor.svelte';

/** An `Api` over an in-memory list of groups, like fmd-server's endpoints. */
function setup(initial: GroupView[], counts: Record<number, number> = {}) {
	let groups = initial.map((g) => ({ ...g }));
	let nextId = 100;
	const api = {
		listGroups: vi.fn(() => Promise.resolve(groups.map((g) => ({ ...g })))),
		createGroup: vi.fn((name: string, rule: RuleView | null = null) => {
			if (groups.some((g) => g.name.toLowerCase() === name.toLowerCase())) {
				return Promise.reject(new ApiError(409, 'createGroup', 'taken'));
			}
			const group: GroupView = { id: nextId++, name, collapsed: false, rule };
			groups.push(group);
			return Promise.resolve({ ...group });
		}),
		updateGroup: vi.fn((id: number, patch: GroupPatch) => {
			const group = groups.find((g) => g.id === id);
			if (!group) return Promise.reject(new ApiError(404, 'updateGroup'));
			if (patch.name != null) group.name = patch.name;
			if (patch.rule !== undefined) group.rule = patch.rule;
			return Promise.resolve({ ...group });
		}),
		deleteGroup: vi.fn((id: number) => {
			groups = groups.filter((g) => g.id !== id);
			return Promise.resolve();
		}),
		reorderGroups: vi.fn((ids: number[]) => {
			groups = ids.flatMap((id) => groups.filter((g) => g.id === id));
			return Promise.resolve();
		})
	};
	const onchange = vi.fn();
	render(GroupEditor, { api: api as unknown as Api, counts, onchange });
	return { api, onchange };
}

const SHOUNEN: GroupView = { id: 1, name: 'Shounen', collapsed: false, rule: null };
const SEINEN: GroupView = { id: 2, name: 'Seinen', collapsed: true, rule: null };

/** The listed groups' names with their counts, in order. */
async function listed(): Promise<string[]> {
	const list = await screen.findByRole('list', { name: 'Groups' });
	return within(list)
		.queryAllByRole('listitem')
		.map((item) => item.querySelector('.name')?.textContent?.trim() ?? '');
}

describe('GroupEditor', () => {
	it('lists the groups with their series counts', async () => {
		setup([SHOUNEN, SEINEN], { 1: 12 });
		await waitFor(async () => expect(await listed()).toEqual(['Shounen · 12', 'Seinen · 0']));
	});

	it('adds a group and says when the name is taken', async () => {
		const { api, onchange } = setup([SHOUNEN]);
		await waitFor(async () => expect(await listed()).toEqual(['Shounen · 0']));

		const name = screen.getByRole('textbox', { name: 'New group' });
		await fireEvent.input(name, { target: { value: 'Seinen' } });
		await fireEvent.click(screen.getByRole('button', { name: 'Add' }));
		expect(api.createGroup).toHaveBeenCalledWith('Seinen', null);
		await waitFor(async () => expect(await listed()).toEqual(['Shounen · 0', 'Seinen · 0']));
		expect(onchange).toHaveBeenCalled();

		await fireEvent.input(name, { target: { value: 'shounen' } });
		await fireEvent.click(screen.getByRole('button', { name: 'Add' }));
		expect(await screen.findByRole('alert')).toHaveProperty(
			'textContent',
			'A group named shounen exists already.'
		);
	});

	it('renames a group inline', async () => {
		const { api } = setup([SHOUNEN]);
		await fireEvent.click(await screen.findByRole('button', { name: 'Edit Shounen' }));

		const name = screen.getByRole('textbox', { name: 'Name of Shounen' });
		await fireEvent.input(name, { target: { value: 'Action' } });
		await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
		expect(api.updateGroup).toHaveBeenCalledWith(1, { name: 'Action', rule: null });
		await waitFor(async () => expect(await listed()).toEqual(['Action · 0']));
	});

	it('adds a group that collects series by tag, and one by format', async () => {
		const { api } = setup([]);
		const name = await screen.findByRole('textbox', { name: 'New group' });
		const rule = screen.getByRole('combobox', { name: 'Auto-add' });
		expect(
			Array.from((rule as HTMLSelectElement).options).map((o) => o.textContent?.trim())
		).toEqual(['No rule', 'Tag', 'Format']);

		await fireEvent.input(name, { target: { value: 'Romance' } });
		await fireEvent.change(rule, { target: { value: 'tag' } });
		await fireEvent.input(screen.getByRole('textbox', { name: 'Tag' }), {
			target: { value: ' Romance ' }
		});
		await fireEvent.click(screen.getByRole('button', { name: 'Add' }));
		expect(api.createGroup).toHaveBeenCalledWith('Romance', { kind: 'tag', value: 'Romance' });
		await waitFor(async () => expect(await listed()).toEqual(['Romance · 0 · auto: tag Romance']));

		await fireEvent.input(name, { target: { value: 'Webtoons' } });
		await fireEvent.change(screen.getByRole('combobox', { name: 'Auto-add' }), {
			target: { value: 'format' }
		});
		const format = screen.getByRole('combobox', { name: 'Format' });
		expect(
			Array.from((format as HTMLSelectElement).options).map((o) => o.textContent?.trim())
		).toEqual(['Manga', 'Manhwa', 'Manhua', 'OEL', 'Other']);
		await fireEvent.change(format, { target: { value: 'manhwa' } });
		await fireEvent.click(screen.getByRole('button', { name: 'Add' }));
		expect(api.createGroup).toHaveBeenLastCalledWith('Webtoons', {
			kind: 'format',
			value: 'manhwa'
		});
		await waitFor(async () =>
			expect(await listed()).toEqual([
				'Romance · 0 · auto: tag Romance',
				'Webtoons · 0 · auto: Manhwa'
			])
		);
	});

	it("edits a group's rule and can clear it", async () => {
		const { api } = setup([{ ...SHOUNEN, rule: { kind: 'tag', value: 'Action' } }], { 1: 3 });
		await waitFor(async () => expect(await listed()).toEqual(['Shounen · 3 · auto: tag Action']));
		await fireEvent.click(screen.getByRole('button', { name: 'Edit Shounen' }));

		const tag = screen.getByRole('textbox', { name: 'Tag of Shounen' });
		expect((tag as HTMLInputElement).value).toBe('Action');
		await fireEvent.input(tag, { target: { value: 'Martial Arts' } });
		await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
		expect(api.updateGroup).toHaveBeenCalledWith(1, {
			name: 'Shounen',
			rule: { kind: 'tag', value: 'Martial Arts' }
		});
		await waitFor(async () =>
			expect(await listed()).toEqual(['Shounen · 3 · auto: tag Martial Arts'])
		);

		await fireEvent.click(screen.getByRole('button', { name: 'Edit Shounen' }));
		await fireEvent.change(screen.getByRole('combobox', { name: 'Auto-add of Shounen' }), {
			target: { value: '' }
		});
		await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
		expect(api.updateGroup).toHaveBeenLastCalledWith(1, { name: 'Shounen', rule: null });
		await waitFor(async () => expect(await listed()).toEqual(['Shounen · 3']));
	});

	it('moves a group up and down', async () => {
		const { api } = setup([SHOUNEN, SEINEN, { id: 3, name: 'Dropped', collapsed: false }]);
		await fireEvent.click(await screen.findByRole('button', { name: 'Move Dropped up' }));
		expect(api.reorderGroups).toHaveBeenCalledWith([1, 3, 2]);
		await waitFor(async () =>
			expect(await listed()).toEqual(['Shounen · 0', 'Dropped · 0', 'Seinen · 0'])
		);

		await fireEvent.click(screen.getByRole('button', { name: 'Move Shounen down' }));
		expect(api.reorderGroups).toHaveBeenLastCalledWith([3, 1, 2]);
		expect(
			(screen.getByRole('button', { name: 'Move Dropped up' }) as HTMLButtonElement).disabled
		).toBe(true);
	});

	it('deletes a group after asking', async () => {
		const { api } = setup([SHOUNEN, SEINEN], { 1: 12 });
		await fireEvent.click(await screen.findByRole('button', { name: 'Delete Shounen' }));
		expect(screen.getByText('Delete Shounen? Its 12 series become ungrouped.')).toBeTruthy();
		expect(api.deleteGroup).not.toHaveBeenCalled();

		await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
		expect(api.deleteGroup).toHaveBeenCalledWith(1);
		await waitFor(async () => expect(await listed()).toEqual(['Seinen · 0']));
	});
});
