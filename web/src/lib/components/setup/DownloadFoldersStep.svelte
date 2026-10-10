<script lang="ts">
	import type { MergePatch } from '#lib/api/client.ts';
	import type { Destination, Settings } from '#lib/api/types.ts';
	import DestinationsEditor from '#lib/components/settings/DestinationsEditor.svelte';
	import { destinationErrors } from '#lib/destinations/destinations.ts';
	import { Draft } from '#lib/settings/draft.svelte.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, settings }: StepProps = $props();

	// svelte-ignore state_referenced_locally
	const draft = new Draft<Settings>(settings);

	const errors = $derived(
		destinationErrors((draft.get('saveto.destinations') ?? []) as unknown as Destination[])
	);
	// Shown next to the fields as they are typed; the editor drops them as a row is edited.
	$effect(() => {
		Object.assign(draft.errors, errors);
	});

	/** Whether the server runs in a container, whose folders must be mounted into it. */
	let inContainer = $state(false);
	$effect(() => {
		api
			.about()
			.then((about) => (inContainer = about.in_container))
			// Only the hint depends on it.
			.catch(() => {});
	});

	export function ready(): boolean {
		return Object.keys(errors).length === 0;
	}

	export async function save(): Promise<MergePatch | void> {
		return draft.changes() ?? undefined;
	}
</script>

<p>
	A destination is a named folder downloads are saved in. Add one for each place you keep manga, say
	one for manga and one for manhwa, and choose the default: downloads go there unless the series
	page or the website picks another. Each website can have its own destination (Settings → Website
	modules), and all of this can be changed later in Settings → Save to.
</p>
{#if inContainer}
	<p class="hint small">
		FMD2r runs in a container: each folder must be mounted into it, and the destination given its
		path inside the container (see "Several download folders" in the README's Docker section).
	</p>
{/if}
<DestinationsEditor {draft} checkFolders={(paths) => api.checkFolders(paths)} />

<style>
	p {
		margin: 0;
	}
	.hint {
		padding: var(--sp-2) var(--sp-3);
		border: 1px solid var(--line);
		border-radius: var(--r);
	}
</style>
