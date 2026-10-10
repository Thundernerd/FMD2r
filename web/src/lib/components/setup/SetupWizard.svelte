<script lang="ts">
	import type { Api, MergePatch } from '#lib/api/client.ts';
	import { ApiError } from '#lib/api/client.ts';
	import type { Settings } from '#lib/api/types.ts';
	import { isObject } from '#lib/settings/draft.svelte.ts';
	import type { SetupStep, StepExports } from '#lib/setup/steps.ts';

	let {
		api,
		steps,
		onfinish
	}: {
		api: Api;
		steps: SetupStep[];
		/** Setup is saved and marked completed; open `to` (`/` from the last step's Finish). */
		onfinish: (to: string) => void;
	} = $props();

	let settings = $state<Settings | null>(null);
	let loadError = $state<string | null>(null);
	let index = $state(0);
	let saving = $state(false);
	let saveError = $state<string | null>(null);
	let current = $state<StepExports | undefined>();
	/** The settings paths an FMD2 import during this setup changed. */
	let fromFmd2 = $state<string[]>([]);

	const step = $derived(steps[index]);
	const last = $derived(index === steps.length - 1);
	const ready = $derived(current?.ready?.() ?? true);
	const startsFromFmd2 = $derived(
		step?.paths?.some((p) => fromFmd2.some((c) => c === p || c.startsWith(`${p}.`))) ?? false
	);

	$effect(() => {
		api
			.getSettings()
			.then((loaded) => {
				// Resumes where it was left; a finished setup starts over.
				index = Math.max(
					steps.findIndex((s) => s.id === loaded.general.setup_step),
					0
				);
				settings = loaded;
			})
			.catch((e: unknown) => (loadError = message(e)));
	});

	function message(e: unknown): string {
		if (e instanceof ApiError && e.detail) return e.detail;
		return e instanceof Error ? e.message : String(e);
	}

	/** The leaf paths (arrays count as leaves) where `a` and `b` differ. */
	function changed(a: unknown, b: unknown, path = ''): string[] {
		if (isObject(a) && isObject(b)) {
			const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
			return [...keys].flatMap((k) => changed(a[k], b[k], path ? `${path}.${k}` : k));
		}
		return JSON.stringify(a) === JSON.stringify(b) ? [] : [path];
	}

	/** Reloads the settings an FMD2 import changed, noting what changed. */
	async function imported() {
		const before = settings;
		const after = await api.getSettings();
		fromFmd2 = [...new Set([...fromFmd2, ...changed(before, after)])];
		settings = after;
	}

	function go(to: number) {
		index = to;
		saveError = null;
	}

	/** `patch` with `general` merged with the wizard's own fields. */
	function withGeneral(patch: MergePatch | void, general: MergePatch): MergePatch {
		const base = patch ?? {};
		const own = isObject(base['general']) ? base['general'] : {};
		return { ...base, general: { ...own, ...general } };
	}

	/** Saves the step, then moves to the next one, or with `to` finishes and opens that. */
	async function advance(to?: string) {
		saving = true;
		saveError = null;
		try {
			const patch = await current?.save?.();
			const next = to === undefined ? steps[index + 1] : undefined;
			settings = await api.patchSettings(
				next
					? withGeneral(patch, { setup_step: next.id })
					: withGeneral(patch, { setup_completed: true, setup_step: '' })
			);
			if (next) go(index + 1);
			else onfinish(to ?? '/');
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
			{#if startsFromFmd2}
				<p class="small muted">These start from your FMD2 settings; change what you like.</p>
			{/if}
			{#key step.id}
				<step.component bind:this={current} {api} {settings} finish={advance} {imported} />
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
			<button
				class="btn primary"
				type="button"
				disabled={!ready || saving}
				onclick={() => advance()}
			>
				{last ? 'Finish' : (current?.nextLabel?.() ?? 'Next')}
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
