<script lang="ts">
	import type { Channel, NotificationTest } from '#lib/api/types.ts';
	import type { Draft } from '#lib/settings/draft.svelte.ts';
	import type { Field } from '#lib/settings/fields.ts';
	import { NOTIFICATION_CHANNELS } from '#lib/settings/sections.ts';
	import SettingField from './SettingField.svelte';

	let {
		draft,
		fields,
		test
	}: {
		draft: Draft<object>;
		/** The Notifications section's fields, `notifications.<channel>.<setting>`. */
		fields: Field[];
		/** Sends a test notification with the channel's saved settings. */
		test: (channel: Channel) => Promise<NotificationTest>;
	} = $props();

	const fieldsOf = (channel: Channel) =>
		fields.filter((f) => f.path.startsWith(`notifications.${channel}.`));

	/** A channel with unsaved edits: a test would use the saved settings, not these. */
	const edited = (channel: Channel) => fieldsOf(channel).some((f) => draft.isDirty(f.path));

	let sending = $state<Partial<Record<Channel, boolean>>>({});
	let results = $state<Partial<Record<Channel, string>>>({});

	async function sendTest(channel: Channel) {
		sending[channel] = true;
		results[channel] = '';
		try {
			const result = await test(channel);
			results[channel] = result.delivered
				? 'Sent. Check that it arrived.'
				: `Not delivered: ${result.error ?? 'unknown error'}`;
		} catch {
			results[channel] = 'Could not send the test.';
		} finally {
			sending[channel] = false;
		}
	}
</script>

<p class="small muted intro">
	Pushes the events you pick to services you already use. Each one is off until you turn it on.
</p>
{#each NOTIFICATION_CHANNELS as channel (channel.id)}
	<fieldset class="channel">
		<legend>{channel.name}</legend>
		{#each fieldsOf(channel.id) as field (field.path)}
			<SettingField {field} {draft} />
		{/each}
		<p class="test">
			<button
				class="btn"
				type="button"
				disabled={sending[channel.id] || edited(channel.id)}
				onclick={() => sendTest(channel.id)}
			>
				{sending[channel.id] ? 'Sending…' : 'Send test'}
			</button>
			{#if edited(channel.id)}
				<span class="small muted">Save first to test these settings.</span>
			{/if}
			<span class="small" role="status">{results[channel.id] ?? ''}</span>
		</p>
	</fieldset>
{/each}

<style>
	.intro {
		margin: 0 0 var(--sp-2);
	}
	.channel {
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		margin: 0 0 var(--sp-3);
		padding: var(--sp-2) var(--sp-3);
		min-width: 0;
	}
	legend {
		font-weight: 600;
		padding: 0 var(--sp-1);
	}
	.test {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--sp-2);
		margin: var(--sp-2) 0 0;
	}
</style>
