import { describe, expect, it } from 'vitest';
import { createApi } from '#lib/api/client.ts';
import { createMockBackend } from '#lib/api/mock.ts';

const api = () => createApi({ baseUrl: 'http://fmd2r.test', fetch: createMockBackend().fetch });

describe('API client against the mock backend', () => {
	it('lists the inbox, newest first', async () => {
		const items = await api().listInbox();

		expect(items.length).toBeGreaterThan(1);
		const times = items.map((i) => Date.parse(i.created_at));
		expect(times).toEqual([...times].sort((a, b) => b - a));
		expect(items.some((i) => !i.read)).toBe(true);
	});

	it('marks an inbox item as read', async () => {
		const client = api();
		const [first] = await client.listInbox();
		if (!first) throw new Error('mock inbox is empty');
		expect(first.read).toBe(false);

		await client.markRead(first.id);

		const after = await client.listInbox();
		expect(after.find((i) => i.id === first.id)?.read).toBe(true);
		expect(after.filter((i) => i.id !== first.id)).toEqual(
			(await api().listInbox()).filter((i) => i.id !== first.id)
		);
	});

	it('rejects marking an unknown inbox item', async () => {
		await expect(api().markRead('does-not-exist')).rejects.toThrow(/404/);
	});
});
