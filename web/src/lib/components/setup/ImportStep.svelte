<script lang="ts">
	import { events } from '#lib/app.ts';
	import ImportFlow from '#lib/components/library/ImportFlow.svelte';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, imported: reload }: StepProps = $props();

	let open = $state(false);
	let busy = $state<'check' | 'import' | null>(null);
	let imported = $state(false);

	export function ready(): boolean {
		return busy === null;
	}

	export function nextLabel(): string {
		return imported ? 'Next' : 'Skip';
	}
</script>

<p>Coming from FMD2? Import your library and settings.</p>
{#if open}
	<ImportFlow
		{api}
		job={events.jobs['import']}
		bind:busy
		onimported={() => {
			imported = true;
			// Next's save returns the imported settings too; only the "from FMD2" note is lost.
			reload().catch(() => {});
		}}
	/>
{:else}
	<p class="small muted">
		The library, downloads, website settings and accounts come along, and the next steps start from
		FMD2's download folder, format and websites.
	</p>
	<div>
		<button class="btn primary" type="button" onclick={() => (open = true)}>Import</button>
	</div>
{/if}

<style>
	p {
		margin: 0;
	}
</style>
