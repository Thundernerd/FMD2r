<script lang="ts">
	import type { Api, MergePatch } from '#lib/api/client.ts';
	import { ApiError } from '#lib/api/client.ts';
	import type { Settings } from '#lib/api/types.ts';
	import type { SetupStep, StepExports } from '#lib/setup/steps.ts';

	let {
		api,
		steps,
		onfinish
	}: {
		api: Api;
		steps: SetupStep[];
		/** The last step was saved and setup is marked completed. */
		onfinish: () => void;
	} = $props();

	let settings = $state<Settings | null>(null);
	let loadError = $state<string | null>(null);
	let index = $state(0);
	let saving = $state(false);
	let saveError = $state<string | null>(null);
	let current = $state<StepExports | undefined>();

	const step = $derived(steps[index]);
	const last = $derived(index === steps.length - 1);
	const ready = $derived(current?.ready?.() ?? true);

	$effect(() => {
		api
			.getSettings()
			.then((loaded) => {
				// A setup run again starts over; an unfinished one resumes where it was left.
				const resume = loaded.general.setup_completed
					? 0
					: steps.findIndex((s) => s.id === loaded.general.setup_step);
				index = Math.max(resume, 0);
				settings = loaded;
			})
			.catch((e: unknown) => (loadError = message(e)));
	});

	function message(e: unknown): string {
		if (e instanceof ApiError && e.detail) return e.detail;
		return e instanceof Error ? e.message : String(e);
	}

	function go(to: number) {
		index = to;
		saveError = null;
	}

	/** `patch` with `general` merged with the wizard's own fields. */
	function withGeneral(patch: MergePatch | void, general: Record<string, unknown>): MergePatch {
		const base = (patch ?? {}) as Record<string, unknown>;
		const own = (base['general'] ?? {}) as Record<string, unknown>;
		return { ...base, general: { ...own, ...general } } as MergePatch;
	}

	async function advance() {
		saving = true;
		saveError = null;
		try {
			const patch = await current?.save?.();
			const next = steps[index + 1];
			settings = await api.patchSettings(
				next
					? withGeneral(patch, { setup_step: next.id })
					: withGeneral(patch, { setup_completed: true, setup_step: '' })
			);
			if (next) go(index + 1);
			else onfinish();
		} catch (e) {
			saveError = message(e);
		} finally {
			saving = false;
		}
	}
</script>

<div class="wizard">
	{#if loadError}
		<p class="error" role="alert">Could not load the settings: {loadError}</p>
	{:else if settings && step}
		<nav class="steps" aria-label="Setup steps">
			<p class="small muted">Step {index + 1} of {steps.length}</p>
			<ol>
				{#each steps as s, i (s.id)}
					<li
						class:done={i < index}
						class:current={i === index}
						aria-current={i === index ? 'step' : undefined}
					>
						{s.title}
					</li>
				{/each}
			</ol>
		</nav>

		<section class="card step" aria-labelledby="setup-step-title">
			<h2 id="setup-step-title">{step.title}</h2>
			{#key step.id}
				<step.component bind:this={current} {api} {settings} />
			{/key}
			{#if saveError}
				<p class="error" role="alert">{saveError}</p>
			{/if}
		</section>

		<div class="actions">
			{#if index > 0}
				<button class="btn" type="button" disabled={saving} onclick={() => go(index - 1)}>
					Back
				</button>
			{/if}
			<button class="btn primary" type="button" disabled={!ready || saving} onclick={advance}>
				{last ? 'Finish' : 'Next'}
			</button>
		</div>
	{/if}
</div>

<style>
	.wizard {
		display: flex;
		flex-direction: column;
		gap: var(--sp-4);
		max-width: 720px;
		margin: 0 auto;
	}
	.steps ol {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-2);
		margin: var(--sp-1) 0 0;
		padding: 0;
		list-style: none;
	}
	.steps li {
		padding: var(--sp-1) var(--sp-2);
		border-radius: var(--r-pill);
		font-size: var(--fs-sm);
		color: var(--muted);
		border: 1px solid var(--line);
		opacity: 0.7;
	}
	.steps li.done {
		opacity: 1;
	}
	.steps li.current {
		opacity: 1;
		font-weight: 600;
		border-color: var(--accent);
	}
	.step {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
	}
	.step h2 {
		margin: 0;
	}
	.actions {
		display: flex;
		justify-content: flex-end;
		gap: var(--sp-2);
	}
	.error {
		color: var(--bad);
		margin: 0;
	}
</style>
