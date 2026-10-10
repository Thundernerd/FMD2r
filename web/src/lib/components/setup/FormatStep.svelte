<script lang="ts">
	import type { MergePatch } from '#lib/api/client.ts';
	import { OUTPUT_FORMATS } from '#lib/settings/sections.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { settings, finish }: StepProps = $props();

	// svelte-ignore state_referenced_locally
	let format = $state(settings.output.format);

	export async function save(): Promise<MergePatch> {
		return { output: { format } };
	}
</script>

<p>How should chapters be saved? You can change this later in Settings.</p>
<fieldset class="formats">
	<legend class="visually-hidden">Save chapters as</legend>
	{#each OUTPUT_FORMATS as choice (choice.value)}
		<label class="format" class:selected={format === choice.value}>
			<input type="radio" name="output-format" value={choice.value} bind:group={format} />
			<span class="text">
				<span class="name">{choice.label}</span>
				<span class="small muted">{choice.description}</span>
			</span>
		</label>
	{/each}
</fieldset>
<p class="small muted">
	Compression, PDF quality and image conversion are in
	<!-- Finishes first, or the unfinished setup would lead straight back here. -->
	<a
		href="/settings#section-output"
		onclick={(e) => {
			e.preventDefault();
			finish('/settings#section-output');
		}}>Settings → Output</a
	>.
</p>

<style>
	p {
		margin: 0;
	}
	.formats {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		margin: 0;
		padding: 0;
		border: none;
	}
	.format {
		display: flex;
		align-items: flex-start;
		gap: var(--sp-3);
		padding: var(--sp-3);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		cursor: pointer;
	}
	.format.selected {
		border-color: var(--accent);
	}
	.format input {
		margin: 4px 0 0;
	}
	.text {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.name {
		font-weight: 600;
	}
</style>
