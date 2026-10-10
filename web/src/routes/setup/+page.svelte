<script lang="ts">
	import { goto } from '$app/navigation';
	import { api, events, session } from '#lib/app.ts';
	import SetupWizard from '#lib/components/setup/SetupWizard.svelte';
	import { setup } from '#lib/setup/status.svelte.ts';
	import { SETUP_STEPS } from '#lib/setup/wizard.ts';

	async function finish(to: string) {
		setup.completed = true;
		// The password step may have set a password, which the banner and Log out go by.
		await session.checkHealth(api).catch(() => {});
		goto(to);
	}
</script>

<svelte:head><title>Setup · FMD2r</title></svelte:head>

<div class="page setup">
	<h1>Set up FMD2r</h1>
	<SetupWizard {api} store={events} steps={SETUP_STEPS} onfinish={finish} />
</div>

<style>
	.setup {
		max-width: 760px;
	}
</style>
