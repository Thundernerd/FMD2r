// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { ApiError, type Api } from '#lib/api/client.ts';
import type { GroupPatch, GroupView } from '#lib/api/types.ts';
import GroupEditor from './GroupEditor.svelte';

/** An `Api` over an in-memory list of groups, like fmd-server's endpoints. */
function setup(initial: GroupView[], counts: Record<number, number> = {}) {
	let groups = initial.map((g) => ({ ...g }));
	let nextId = 100;
	const api = {
		listGroups: vi.fn(() => Promise.resolve(groups.map((g) => ({ ...g })))),
		createGroup: vi.fn((name: string) => {
			if (groups.some((g) => g.name.toLowerCase() === name.toLowerCase())) {
				return Promise.reject(new ApiError(409, 'createGroup', 'taken'));
			}
			const group = { id: nextId++, name, collapsed: false };
			groups.push(group);
			return Promise.resolve({ ...group });
		}),
		updateGroup: vi.fn((id: number, patch: GroupPatch) => {
			const group = groups.find((g) => g.id === id);
			if (!group) return Promise.reject(new ApiError(404, 'updateGroup'));
			if (patch.name != null) group.name = patch.name;
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

const SHOUNEN = { id: 1, name: 'Shounen', collapsed: false };
const SEINEN = { id: 2, name: 'Seinen', collapsed: true };

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
		expect(api.createGroup).toHaveBeenCalledWith('Seinen');
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
		await fireEvent.click(await screen.findByRole('button', { name: 'Rename Shounen' }));

		const name = screen.getByRole('textbox', { name: 'Name of Shounen' });
		await fireEvent.input(name, { target: { value: 'Action' } });
		await fireEvent.click(screen.getByRole('button', { name: 'Save' }));
		expect(api.updateGroup).toHaveBeenCalledWith(1, { name: 'Action' });
		await waitFor(async () => expect(await listed()).toEqual(['Action · 0']));
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
