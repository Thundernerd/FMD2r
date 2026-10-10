<script lang="ts">
	import type { MergePatch } from '#lib/api/client.ts';
	import type { ModuleSummary } from '#lib/api/types.ts';
	import WebsiteSelection, {
		selectedWebsites
	} from '#lib/components/settings/WebsiteSelection.svelte';
	import { Draft } from '#lib/settings/draft.svelte.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, settings }: StepProps = $props();

	let modules = $state<ModuleSummary[] | null>(null);
	let failed = $state(false);

	function load() {
		failed = false;
		api
			.listModules()
			.then((loaded) => (modules = loaded))
			.catch(() => (failed = true));
	}
	$effect(load);

	// svelte-ignore state_referenced_locally
	const draft = new Draft<object>({
		general: { selected_websites: settings.general.selected_websites }
	});

	const selection = $derived(selectedWebsites(draft));
	/** Whether a loaded website is selected, so Discover has one to list. */
	const hasWebsite = $derived(modules?.some((m) => selection.includes(m.id)) ?? false);

	export function ready(): boolean {
		return hasWebsite;
	}

	export async function save(): Promise<MergePatch> {
		return { general: { selected_websites: selection } };
	}
</script>

{#if failed}
	<p class="error" role="alert">Could not load the websites.</p>
	<button class="btn sm" type="button" onclick={load}>Try again</button>
{:else if modules}
	<WebsiteSelection {modules} {draft} />
	{#if !hasWebsite}
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
	.btn {
		align-self: flex-start;
	}
</style>
