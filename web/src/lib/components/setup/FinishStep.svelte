<script lang="ts">
	import type { MangaBakaStatus } from '#lib/api/types.ts';
	import { SETTINGS_SECTIONS } from '#lib/settings/sections.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, settings, finish }: StepProps = $props();

	let mangabaka = $state<MangaBakaStatus | null>(null);
	$effect(() => {
		api
			.mangabakaStatus()
			.then((status) => (mangabaka = status))
			.catch(() => {});
	});

	/** The label Settings shows for the output format. */
	const formatLabel = $derived.by(() => {
		const control = SETTINGS_SECTIONS.flatMap((s) => s.fields).find(
			(f) => f.path === 'output.format'
		)?.control;
		const choice =
			control?.kind === 'select'
				? control.choices.find((c) => c.value === settings.output.format)
				: undefined;
		return choice?.label ?? settings.output.format;
	});

	const folders = $derived(
		settings.saveto.destinations.map((d) => `${d.name} (${d.path})${d.default ? ', default' : ''}`)
	);
	const websites = $derived(settings.general.selected_websites.length);

	interface Choice {
		label: string;
		value: string;
		href: string;
	}
	const choices: Choice[] = $derived([
		{
			label: 'Download folders',
			value: folders.join('; '),
			href: '/settings#section-saveto'
		},
		{ label: 'Download format', value: formatLabel, href: '/settings#section-output' },
		{
			label: 'Websites',
			value: websites === 0 ? 'None yet' : `${websites} selected`,
			href: '/settings#section-websites'
		},
		{
			label: 'MangaBaka database',
			value: mangabaka?.downloaded
				? 'Downloaded'
				: mangabaka?.running
					? 'Still downloading; Settings shows its progress'
					: 'Not downloaded',
			href: '/settings#section-metadata'
		}
	]);
</script>

<p>FMD2r is ready. This is what you chose:</p>
<dl class="summary">
	{#each choices as choice (choice.label)}
		<div class="row">
			<dt>{choice.label}</dt>
			<dd>
				<span>{choice.value}</span>
				<!-- Finishes first, or the unfinished setup would lead straight back here. -->
				<a
					class="small"
					href={choice.href}
					aria-label="Change {choice.label} in Settings"
					onclick={(e) => {
						e.preventDefault();
						finish(choice.href);
					}}>Change in Settings</a
				>
			</dd>
		</div>
	{/each}
</dl>
<p class="small muted">Finish opens your library.</p>

<style>
	p {
		margin: 0;
	}
	.summary {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		margin: 0;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-1) var(--sp-3);
		padding: var(--sp-2) 0;
		border-bottom: 1px solid var(--line);
	}
	dt {
		flex: 0 0 180px;
		font-weight: 600;
	}
	dd {
		flex: 1 1 240px;
		display: flex;
		flex-wrap: wrap;
		justify-content: space-between;
		gap: var(--sp-2);
		margin: 0;
		min-width: 0;
		overflow-wrap: anywhere;
	}
</style>
