<script lang="ts">
	import type { Draft, Json } from '#lib/settings/draft.svelte.ts';
	import { choicesOf, secretFlag, type Field } from '#lib/settings/fields.ts';

	let {
		field,
		draft,
		idPrefix = 'set',
		overridden = false
	}: {
		field: Field;
		draft: Draft<object>;
		idPrefix?: string;
		/** The command line or environment overrides the setting. */
		overridden?: boolean;
	} = $props();

	const id = $derived(`${idPrefix}-${field.path.replaceAll('.', '-')}`);
	const value = $derived(draft.get(field.path));
	const error = $derived(draft.errors[field.path]);
	const dirty = $derived(draft.isDirty(field.path));
	/** Swatches that can't be changed now, and why (`lockedWhen` of the control). */
	const lockNote = $derived.by(() => {
		const control = field.control;
		if (control.kind !== 'swatches' || !control.lockedWhen) return null;
		const { path, value, note } = control.lockedWhen;
		return draft.get(path) === value ? note : null;
	});
	const describedBy = $derived(
		[
			error ? `${id}-error` : '',
			field.help ? `${id}-help` : '',
			overridden ? `${id}-overridden` : '',
			lockNote ? `${id}-locked` : ''
		]
			.filter(Boolean)
			.join(' ') || undefined
	);
	/** The accent the theme cards show, the one picked with the swatches. */
	const accent = $derived.by(() => {
		const control = field.control;
		return control.kind === 'themes' ? draft.get(control.accentPath) : undefined;
	});

	/** Whether the secret is set on the server; it never sends the value. */
	const secretSet = $derived(draft.get(secretFlag(field.path)) === true);
	const secretStatus = $derived.by(() => {
		if (value === '') return 'Cleared when saved';
		if (typeof value === 'string') return 'Changed when saved';
		return secretSet ? 'Set' : 'Not set';
	});

	/** Erasing what was typed leaves the stored secret as it is. */
	function onSecret(raw: string) {
		if (raw === '') draft.unset(field.path);
		else draft.set(field.path, raw);
	}

	function onText(raw: string) {
		const control = field.control;
		draft.set(field.path, control.kind === 'text' && control.nullable && raw === '' ? null : raw);
	}

	/**
	 * An empty box clears a nullable number. Any other number needs a value: `null` would reset
	 * it to its default, so the field keeps its value and reports the error instead.
	 */
	function onNumber(raw: string) {
		const control = field.control;
		if (raw === '' && !(control.kind === 'number' && control.nullable)) {
			draft.errors[field.path] = 'Enter a number.';
			return;
		}
		draft.set(field.path, raw === '' ? null : Number(raw));
	}

	/** Arrow keys move the pick along the row, as in any radio group. */
	function onRadioKey(e: KeyboardEvent, index: number) {
		const choices = choicesOf(field.control);
		const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
		if (!step || !choices.length) return;
		e.preventDefault();
		const count = choices.length;
		const next = (index + step + count) % count;
		const choice = choices[next];
		if (!choice) return;
		draft.set(field.path, choice.value as Json);
		const row = (e.currentTarget as HTMLElement).parentElement;
		row?.querySelectorAll<HTMLElement>('[role="radio"]')[next]?.focus();
	}

	function onSelect(index: number) {
		const control = field.control;
		if (control.kind !== 'select') return;
		const choice = control.choices[index];
		if (choice) draft.set(field.path, choice.value as Json);
	}
</script>

<div class="field" class:dirty class:invalid={!!error} data-path={field.path}>
	{#if field.control.kind === 'checkbox'}
		<label class="check" for={id}>
			<input
				{id}
				type="checkbox"
				checked={value === true}
				aria-invalid={error ? true : undefined}
				aria-describedby={describedBy}
				onchange={(e) => draft.set(field.path, e.currentTarget.checked)}
			/>
			<span>{field.label}</span>
		</label>
	{:else if field.control.kind === 'swatches' || field.control.kind === 'themes'}
		{@const kind = field.control.kind}
		{@const checked = field.control.choices.findIndex((c) => c.value === value)}
		<span class="caption" id="{id}-label">{field.label}</span>
		<div
			{id}
			class={kind}
			role="radiogroup"
			aria-labelledby="{id}-label"
			aria-invalid={error ? true : undefined}
			aria-describedby={describedBy}
			aria-disabled={lockNote ? true : undefined}
		>
			{#each field.control.choices as choice, i (choice.value)}
				<button
					type="button"
					role="radio"
					class={kind === 'swatches' ? 'swatch' : 'theme'}
					data-accent={kind === 'swatches' ? choice.value : undefined}
					title={kind === 'swatches' ? choice.label : undefined}
					aria-label={kind === 'swatches' ? choice.label : undefined}
					aria-checked={i === checked}
					tabindex={i === checked || (checked < 0 && i === 0) ? 0 : -1}
					disabled={!!lockNote}
					onclick={() => draft.set(field.path, choice.value as Json)}
					onkeydown={(e) => onRadioKey(e, i)}
				>
					{#if kind === 'themes'}
						<!-- The theme's own background, surface, text and accent (`data-style` in
						     tokens.css), over the default theme and the picked accent rather than the
						     theme in use, whose accent may be its own. -->
						<span
							class="theme-preview"
							data-style="default"
							data-accent={typeof accent === 'string' ? accent : undefined}
							aria-hidden="true"
						>
							<span class="theme-bg" data-style={choice.value}>
								<span class="theme-surface">
									<span class="theme-text">Aa</span>
									<span class="theme-accent"></span>
								</span>
							</span>
						</span>
						<span class="theme-name">{choice.label}</span>
					{/if}
				</button>
			{/each}
		</div>
		{#if lockNote}
			<p class="locked small muted" id="{id}-locked">{lockNote}</p>
		{/if}
	{:else}
		<label class="caption" for={id}>{field.label}</label>
		{#if field.control.kind === 'text'}
			<input
				{id}
				class="input"
				type="text"
				autocomplete="off"
				value={typeof value === 'string' ? value : ''}
				placeholder={field.control.placeholder}
				aria-invalid={error ? true : undefined}
				aria-describedby={describedBy}
				oninput={(e) => onText(e.currentTarget.value)}
			/>
		{:else if field.control.kind === 'secret'}
			<div class="secret">
				<input
					{id}
					class="input"
					type="password"
					autocomplete="new-password"
					value={typeof value === 'string' ? value : ''}
					placeholder={secretSet ? 'Unchanged' : ''}
					aria-invalid={error ? true : undefined}
					aria-describedby={describedBy}
					oninput={(e) => onSecret(e.currentTarget.value)}
				/>
				<span class="small muted">{secretStatus}</span>
				{#if secretSet && value === undefined}
					<button type="button" class="btn sm" onclick={() => draft.set(field.path, '')}>
						Clear
					</button>
				{/if}
			</div>
		{:else if field.control.kind === 'number'}
			<input
				{id}
				class="input num"
				type="number"
				inputmode="numeric"
				step="1"
				min={field.control.min}
				max={field.control.max}
				value={typeof value === 'number' ? value : ''}
				aria-invalid={error ? true : undefined}
				aria-describedby={describedBy}
				oninput={(e) => onNumber(e.currentTarget.value)}
			/>
		{:else if field.control.kind === 'select'}
			<select
				{id}
				class="input"
				value={field.control.choices.findIndex((c) => c.value === value)}
				aria-invalid={error ? true : undefined}
				aria-describedby={describedBy}
				onchange={(e) => onSelect(Number(e.currentTarget.value))}
			>
				{#each field.control.choices as choice, i (i)}
					<option value={i}>{choice.label}</option>
				{/each}
			</select>
		{/if}
	{/if}
	{#if field.help}
		<p id="{id}-help" class="help small muted">{field.help}</p>
	{/if}
	{#if overridden}
		<p id="{id}-overridden" class="overridden small">
			Overridden by the command line or environment; this value applies without it.
		</p>
	{/if}
	{#if error}
		<p id="{id}-error" class="field-error small" role="alert">{error}</p>
	{/if}
</div>

<style>
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
		padding: var(--sp-2) 0 var(--sp-2) var(--sp-3);
		border-left: 2px solid transparent;
	}
	.field.dirty {
		border-left-color: var(--accent);
	}
	.field.invalid {
		border-left-color: var(--bad);
	}
	.caption {
		font-weight: 500;
	}
	.check {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		font-weight: 500;
	}
	.input {
		max-width: 480px;
	}
	.secret {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.swatches {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-2);
	}
	/* Each swatch is its own accent: `data-accent` sets `--accent` on it (tokens.css). */
	.swatch {
		width: 32px;
		height: 32px;
		padding: 0;
		border-radius: var(--r-pill);
		border: 2px solid var(--surface);
		background: var(--accent);
		box-shadow: 0 0 0 1px var(--line);
		cursor: pointer;
	}
	.swatch[aria-checked='true'] {
		box-shadow: 0 0 0 2px var(--fg);
	}
	.swatch:disabled {
		opacity: 0.35;
		cursor: not-allowed;
	}
	.locked {
		margin: 0;
	}
	.themes {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-3);
	}
	/* Each card's preview shows its own theme: `data-style` sets the theme's tokens on it (tokens.css). */
	.theme {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
		width: 120px;
		padding: var(--sp-1);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		background: var(--surface);
		color: var(--fg);
		font: var(--fs-ui) / var(--lh) var(--f-body);
		text-align: left;
		cursor: pointer;
	}
	.theme[aria-checked='true'] {
		border-color: var(--fg);
		box-shadow: 0 0 0 1px var(--fg);
	}
	.theme-preview {
		display: flex;
		flex-direction: column;
	}
	.theme-bg {
		display: flex;
		padding: var(--sp-2);
		border-radius: var(--r);
		background: var(--bg);
	}
	.theme-surface {
		display: flex;
		flex: 1;
		align-items: center;
		justify-content: space-between;
		padding: var(--sp-1) var(--sp-2);
		border: 1px solid var(--line);
		border-radius: var(--r);
		background: var(--surface);
	}
	.theme-text {
		font: 700 var(--fs-lg) / 1 var(--f-display);
		color: var(--fg);
	}
	.theme-accent {
		width: 16px;
		height: 16px;
		border-radius: var(--r-pill);
		background: var(--accent);
	}
	.theme-name {
		padding: 0 var(--sp-1);
		font-weight: 500;
	}
	.input.num {
		max-width: 140px;
	}
	.invalid .input {
		border-color: var(--bad);
	}
	.help {
		margin: 0;
	}
	.overridden {
		margin: 0;
		color: var(--warn);
	}
	.field-error {
		margin: 0;
		color: var(--bad);
	}
</style>
