// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import type { Api } from '#lib/api/client.ts';
import type { InboxItem } from '#lib/api/types.ts';
import { EventStore } from '#lib/events.svelte.ts';
import { inboxUi } from '#lib/inbox.svelte.ts';
import InboxPopover from './InboxPopover.svelte';

const item = (id: string, read: boolean): InboxItem => ({
	id,
	kind: 'warn',
	title: `item ${id}`,
	body: '',
	created_at: '2026-10-09T12:00:00Z',
	read
});

/** Waits for the popover's API call and the store update after it. */
const settle = async () => {
	await Promise.resolve();
	await Promise.resolve();
	flushSync();
};

describe('InboxPopover', () => {
	let component: ReturnType<typeof mount> | null = null;

	afterEach(() => {
		if (component) unmount(component);
		component = null;
		inboxUi.open = false;
		document.body.innerHTML = '';
	});

	it('renders a multi-line body on separate lines', () => {
		const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
		store.seed({
			inbox: [
				{
					id: '1',
					kind: 'error',
					title: 'module Broken.lua failed Init',
					body: 'modules/Broken.lua:3: boom\nstack traceback:\n\t[C]: in ?',
					created_at: '2026-10-09T12:00:00Z',
					read: false
				}
			]
		});
		inboxUi.open = true;
		// The popover only reads `api` when marking an item read, which this test doesn't do.
		component = mount(InboxPopover, { target: document.body, props: { api: {} as Api, store } });
		flushSync();

		const lines = [...document.querySelectorAll('.body .line')].map((l) => l.textContent);
		expect(lines).toEqual(['modules/Broken.lua:3: boom', 'stack traceback:', '\t[C]: in ?']);
	});

	function open(items: InboxItem[], api: Partial<Api>) {
		const store = new EventStore({ url: '/api/events', connect: () => ({}) as never });
		store.seed({ inbox: items });
		inboxUi.open = true;
		component = mount(InboxPopover, {
			target: document.body,
			props: { api: api as Api, store }
		});
		flushSync();
		return store;
	}

	const button = (name: string): HTMLButtonElement => {
		const found = [...document.querySelectorAll('button')].find(
			(b) => b.textContent?.trim() === name
		);
		if (!found) throw new Error(`no ${name} button`);
		return found;
	};

	it('marks every item read at once', async () => {
		let calls = 0;
		const store = open([item('1', false), item('2', false), item('3', true)], {
			markAllRead: async () => {
				calls++;
			}
		});

		button('Mark all read').click();
		await settle();

		expect(calls).toBe(1);
		expect(document.querySelectorAll('.item.unread')).toHaveLength(0);
		expect(document.querySelector('.badge')).toBeNull();
		expect(store.unread).toBe(0);
	});

	it('deletes one item', async () => {
		const deleted: string[] = [];
		const store = open([item('1', false), item('2', true)], {
			deleteInboxItem: async (id) => {
				deleted.push(id);
			}
		});

		const first = document.querySelectorAll('.item')[0];
		const del = [...(first?.querySelectorAll('button') ?? [])].find(
			(b) => b.textContent?.trim() === 'Delete'
		);
		del?.click();
		await settle();

		expect(deleted).toEqual(['1']);
		const titles = [...document.querySelectorAll('.item .title')].map((t) => t.textContent);
		expect(titles).toEqual(['item 2']);
		expect(store.unread).toBe(0);
	});

	it('clears the read items', async () => {
		let calls = 0;
		open([item('1', true), item('2', false), item('3', true)], {
			clearReadInbox: async () => {
				calls++;
			}
		});

		button('Clear read').click();
		await settle();

		expect(calls).toBe(1);
		const titles = [...document.querySelectorAll('.item .title')].map((t) => t.textContent);
		expect(titles).toEqual(['item 2']);
	});

	it('disables the header buttons when there is nothing to act on', () => {
		open([item('1', false)], {});
		expect(button('Mark all read').disabled).toBe(false);
		expect(button('Clear read').disabled).toBe(true);
		if (component) unmount(component);

		open([item('1', true)], {});
		expect(button('Mark all read').disabled).toBe(true);
		expect(button('Clear read').disabled).toBe(false);
	});
});
