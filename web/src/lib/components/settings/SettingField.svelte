<script lang="ts">
	import type { Draft, Json } from '#lib/settings/draft.svelte.ts';
	import type { Field } from '#lib/settings/fields.ts';

	let {
		field,
		draft,
		idPrefix = 'set'
	}: { field: Field; draft: Draft<object>; idPrefix?: string } = $props();

	const id = $derived(`${idPrefix}-${field.path.replaceAll('.', '-')}`);
	const value = $derived(draft.get(field.path));
	const error = $derived(draft.errors[field.path]);
	const dirty = $derived(draft.isDirty(field.path));
	const describedBy = $derived(
		[error ? `${id}-error` : '', field.help ? `${id}-help` : ''].filter(Boolean).join(' ') ||
			undefined
	);

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
	{:else}
		<label class="caption" for={id}>{field.label}</label>
		{#if field.control.kind === 'text'}
			<input
				{id}
				class="input"
				type={field.control.secret ? 'password' : 'text'}
				autocomplete="off"
				value={typeof value === 'string' ? value : ''}
				placeholder={field.control.placeholder}
				aria-invalid={error ? true : undefined}
				aria-describedby={describedBy}
				oninput={(e) => onText(e.currentTarget.value)}
			/>
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
	.input.num {
		max-width: 140px;
	}
	.invalid .input {
		border-color: var(--bad);
	}
	.help {
		margin: 0;
	}
	.field-error {
		margin: 0;
		color: var(--bad);
	}
</style>
