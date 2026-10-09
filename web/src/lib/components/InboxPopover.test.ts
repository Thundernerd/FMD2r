// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import type { Api } from '#lib/api/client.ts';
import { EventStore } from '#lib/events.svelte.ts';
import { inboxUi } from '#lib/inbox.svelte.ts';
import InboxPopover from './InboxPopover.svelte';

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
});
