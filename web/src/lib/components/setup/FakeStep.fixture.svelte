<script lang="ts">
	import type { MergePatch } from '#lib/api/client.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	// A setup step for the wizard's tests: edits the UI language, can be made not ready, and can
	// save a value the server rejects, or finish the setup from within. It shows the download
	// format it starts from, as a step that edits it would.
	let { settings, finish }: StepProps = $props();

	// svelte-ignore state_referenced_locally
	let language = $state(settings.general.language);
	let blocked = $state(false);
	let invalid = $state(false);

	export function ready(): boolean {
		return !blocked;
	}

	export async function save(): Promise<MergePatch> {
		return invalid ? { connections: { timeout_secs: 0 } } : { general: { language } };
	}
</script>

<label>Language <input bind:value={language} /></label>
<label>Format <input value={settings.output.format} readonly /></label>
<label><input type="checkbox" bind:checked={blocked} /> Not ready</label>
<label><input type="checkbox" bind:checked={invalid} /> Invalid value</label>
<button type="button" onclick={() => finish('/settings#section-general')}>Finish here</button>
