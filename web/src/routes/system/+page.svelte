<script lang="ts">
	import { api, events } from '#lib/app.ts';
	import AboutPanel from '#lib/components/system/AboutPanel.svelte';
	import JobsPanel from '#lib/components/system/JobsPanel.svelte';
	import LogsPanel from '#lib/components/system/LogsPanel.svelte';

	const TABS = [
		{ id: 'logs', label: 'Logs' },
		{ id: 'jobs', label: 'Jobs' },
		{ id: 'about', label: 'About' }
	] as const;
	type Tab = (typeof TABS)[number]['id'];

	let tab = $state<Tab>('logs');
	let tablist: HTMLElement | undefined = $state();

	/** Arrow keys move between tabs (WAI-ARIA tabs pattern). */
	function onKeydown(event: KeyboardEvent) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step) return;
		const index = TABS.findIndex((t) => t.id === tab);
		const next = TABS[(index + step + TABS.length) % TABS.length];
		if (!next) return;
		tab = next.id;
		tablist?.querySelector<HTMLElement>(`#tab-${next.id}`)?.focus();
	}
</script>

<svelte:head><title>System · FMD2r</title></svelte:head>

<div class="page">
	<h1>System</h1>

	<div
		class="tabs"
		role="tablist"
		aria-label="System sections"
		tabindex="-1"
		bind:this={tablist}
		onkeydown={onKeydown}
	>
		{#each TABS as t (t.id)}
			<button
				id="tab-{t.id}"
				class="tab"
				type="button"
				role="tab"
				aria-selected={tab === t.id}
				aria-controls="panel-{t.id}"
				tabindex={tab === t.id ? 0 : -1}
				onclick={() => (tab = t.id)}>{t.label}</button
			>
		{/each}
	</div>

	<div id="panel-{tab}" role="tabpanel" aria-labelledby="tab-{tab}">
		{#if tab === 'logs'}
			<LogsPanel {api} store={events} />
		{:else if tab === 'jobs'}
			<JobsPanel {api} store={events} />
		{:else}
			<AboutPanel {api} />
		{/if}
	</div>
</div>

<style>
	.tabs {
		display: flex;
		gap: var(--sp-1);
		border-bottom: 1px solid var(--line);
	}
	.tab {
		border: 0;
		background: transparent;
		padding: var(--sp-2) var(--sp-3);
		color: var(--muted);
		border-bottom: 2px solid transparent;
		margin-bottom: -1px;
		font-weight: 500;
	}
	.tab[aria-selected='true'] {
		color: var(--fg);
		border-bottom-color: var(--accent);
	}
	.tab:hover {
		color: var(--fg);
	}
</style>
