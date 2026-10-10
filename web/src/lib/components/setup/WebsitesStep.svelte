<script lang="ts">
	import type { MergePatch } from '#lib/api/client.ts';
	import type { ModuleSummary } from '#lib/api/types.ts';
	import WebsiteSelection from '#lib/components/settings/WebsiteSelection.svelte';
	import { Draft } from '#lib/settings/draft.svelte.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, settings }: StepProps = $props();

	let modules = $state<ModuleSummary[] | null>(null);
	let loadError = $state<string | null>(null);
	$effect(() => {
		api
			.listModules()
			.then((loaded) => (modules = loaded))
			.catch((e: unknown) => (loadError = e instanceof Error ? e.message : String(e)));
	});

	// svelte-ignore state_referenced_locally
	const draft = new Draft<object>({
		general: { selected_websites: settings.general.selected_websites }
	});

	/** The selected websites; a module that is not loaded keeps its place for when it comes back. */
	const selection = $derived.by((): string[] => {
		const value = draft.get('general.selected_websites');
		return Array.isArray(value) ? value.filter((v) => typeof v === 'string') : [];
	});
	/** Whether a loaded website is selected, so Discover has one to list. */
	const chosen = $derived(modules?.some((m) => selection.includes(m.id)) ?? false);

	export function ready(): boolean {
		return chosen;
	}

	export async function save(): Promise<MergePatch> {
		return { general: { selected_websites: selection } };
	}
</script>

<p>
	Choose the websites Discover lists and searches. The library and Add by URL work with every
	website, whichever you choose here.
</p>
{#if loadError}
	<p class="error" role="alert">Could not load the websites: {loadError}</p>
{:else if modules}
	<WebsiteSelection {modules} {draft} />
	{#if !chosen}
		<p class="small muted">Select at least one website to continue, so Discover has one to list.</p>
	{/if}
{:else}
	<p class="small muted">Loading the websites…</p>
{/if}
<p class="small muted">You can change this later in Settings → Websites.</p>

<style>
	p {
		margin: 0;
	}
	.error {
		color: var(--bad);
	}
</style>
