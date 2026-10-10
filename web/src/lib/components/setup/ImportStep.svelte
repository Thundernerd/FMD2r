<script lang="ts">
	import ImportFlow, { type ImportBusy } from '#lib/components/library/ImportFlow.svelte';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, store, reloadSettings }: StepProps = $props();

	let open = $state(false);
	let running = $state<ImportBusy>(null);
	let reloading = $state(false);
	let imported = $state(false);

	export function busy(): boolean {
		return running !== null || reloading;
	}

	export function nextLabel(): string {
		return imported ? 'Next' : 'Skip';
	}
</script>

<p>Coming from FMD2? Import your library and settings.</p>
{#if open}
	<ImportFlow
		{api}
		job={store.jobs['import']}
		bind:busy={running}
		onimported={async () => {
			imported = true;
			reloading = true;
			try {
				await reloadSettings();
			} catch {
				// Next's save returns the imported settings too; only the "from FMD2" note is lost.
			} finally {
				reloading = false;
			}
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
