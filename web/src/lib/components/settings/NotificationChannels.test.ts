// @vitest-environment jsdom
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Channel, NotificationTest } from '#lib/api/types.ts';
import { Draft } from '#lib/settings/draft.svelte.ts';
import { SETTINGS_SECTIONS } from '#lib/settings/sections.ts';
import { defaultSettings } from '#lib/api/mock-settings.ts';
import NotificationChannels from './NotificationChannels.svelte';

const fields = SETTINGS_SECTIONS.find((s) => s.id === 'notifications')?.fields ?? [];

function setup(results: NotificationTest[]) {
	const settings = defaultSettings();
	settings.notifications.ntfy.topic_url = 'https://ntfy.sh/my-topic';
	const draft = new Draft<object>(settings);
	let call = 0;
	const test = vi.fn<(channel: Channel) => Promise<NotificationTest>>(() =>
		Promise.resolve(results[Math.min(call++, results.length - 1)] as NotificationTest)
	);
	render(NotificationChannels, { draft, fields, test });
	return { draft, test };
}

const group = (name: string) => within(screen.getByRole('group', { name }));

describe('NotificationChannels', () => {
	it('sends a test to the channel and says it arrived', async () => {
		const { test } = setup([{ delivered: true, error: null }]);

		await fireEvent.click(group('ntfy').getByRole('button', { name: 'Send test' }));

		expect(test).toHaveBeenCalledWith('ntfy');
		expect(await group('ntfy').findByText('Sent. Check that it arrived.')).toBeTruthy();
	});

	it('says why a test was not delivered', async () => {
		setup([{ delivered: false, error: 'HTTP 500' }]);

		await fireEvent.click(group('Discord').getByRole('button', { name: 'Send test' }));

		expect(await group('Discord').findByText('Not delivered: HTTP 500')).toBeTruthy();
		expect(group('ntfy').queryByRole('status')?.textContent ?? '').toBe('');
	});

	it('tests only saved settings', async () => {
		const { draft, test } = setup([{ delivered: true, error: null }]);
		draft.set('notifications.webhook.url', 'http://n8n.local/webhook/fmd');
		await Promise.resolve();

		const button = group('Webhook').getByRole('button', { name: 'Send test' });
		expect((button as HTMLButtonElement).disabled).toBe(true);
		expect(group('Webhook').getByText('Save first to test these settings.')).toBeTruthy();
		expect(
			(group('ntfy').getByRole('button', { name: 'Send test' }) as HTMLButtonElement).disabled
		).toBe(false);
		expect(test).not.toHaveBeenCalled();
	});

	it('shows one checkbox per notification type under each channel', () => {
		setup([]);

		for (const name of ['ntfy', 'Discord', 'Webhook']) {
			const types = within(group(name).getByRole('group', { name: 'Send these' }));
			expect(
				types.getAllByRole('checkbox').map((c) => c.closest('label')?.textContent?.trim())
			).toEqual([
				'New chapters',
				'Missing chapters',
				'Missing-chapter check found nothing',
				'Completed series removed',
				'Download failed',
				'Library check failed',
				'Module updates',
				'Downloads moved',
				'Comic server rescan'
			]);
			expect(
				(types.getByRole('checkbox', { name: 'Module updates' }) as HTMLInputElement).checked
			).toBe(true);
			expect(
				(types.getByRole('checkbox', { name: 'Completed series removed' }) as HTMLInputElement)
					.checked
			).toBe(false);
		}
	});

	it('marks the channel’s types dirty when one is ticked', async () => {
		const { draft } = setup([]);
		const discord = within(group('Discord').getByRole('group', { name: 'Send these' }));

		await fireEvent.click(discord.getByRole('checkbox', { name: 'Completed series removed' }));

		expect(draft.isDirty('notifications.discord.types')).toBe(true);
		expect(draft.isDirty('notifications.ntfy.types')).toBe(false);
		expect(draft.get('notifications.discord.types')).toContain('completed');
		expect(
			(discord.getByRole('checkbox', { name: 'Completed series removed' }) as HTMLInputElement)
				.checked
		).toBe(true);
	});
});
