<script lang="ts" module>
	import type { RuleView } from '#lib/api/types.ts';
	import { FORMATS, formatLabel } from '#lib/library/formats.ts';

	/** What a group's row says about its rule: `auto: tag Romance`, `auto: Manhwa`. */
	export function describeRule(rule: RuleView): string {
		if (rule.kind === 'tag') return `auto: tag ${rule.value}`;
		return `auto: ${formatLabel(rule.value)}`;
	}

	/** The rule the fields describe; `null` for "No rule", or a tag rule without a tag. */
	export function ruleOf(kind: RuleView['kind'] | '', value: string): RuleView | null {
		const trimmed = value.trim();
		return kind && trimmed ? { kind, value: trimmed } : null;
	}
</script>

<script lang="ts">
	let {
		kind = $bindable(),
		tag = $bindable(),
		format = $bindable(),
		of = null
	}: {
		/** `''` is "No rule". */
		kind: RuleView['kind'] | '';
		tag: string;
		format: string;
		/** The group the fields edit, named in their labels; `null` in the add form. */
		of?: string | null;
	} = $props();

	const label = (what: string) => (of ? `${what} of ${of}` : what);
</script>

<select class="input" aria-label={label('Auto-add')} bind:value={kind}>
	<option value="">No rule</option>
	<option value="tag">Tag</option>
	<option value="format">Format</option>
</select>
{#if kind === 'tag'}
	<input class="input" aria-label={label('Tag')} placeholder="Tag" bind:value={tag} required />
{:else if kind === 'format'}
	<select class="input" aria-label={label('Format')} bind:value={format}>
		{#each FORMATS as [value, name] (value)}
			<option {value}>{name}</option>
		{/each}
	</select>
{/if}
