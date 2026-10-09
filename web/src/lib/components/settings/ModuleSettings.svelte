<script lang="ts">
	import type { ModuleSettingsView, ModuleSummary } from '#lib/api/types.ts';
	import type { Draft } from '#lib/settings/draft.svelte.ts';
	import { moduleHost, moduleKey, repeatedNames } from '#lib/modules.ts';
	import { optionFields, type Field } from '#lib/settings/fields.ts';
	import SettingField from './SettingField.svelte';

	let {
		modules,
		selected,
		view,
		draft,
		loading,
		onselect
	}: {
		modules: ModuleSummary[];
		selected: string | null;
		/** The selected module's settings as last saved, once loaded. */
		view: ModuleSettingsView | null;
		/** Edits to `view`: `enabled`, `limits`, `http` and `options` keyed by option key. */
		draft: Draft<object> | null;
		loading: boolean;
		onselect: (id: string) => void;
	} = $props();

	let query = $state('');
	const matches = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return q
			? modules.filter((m) => m.name.toLowerCase().includes(q) || m.id.toLowerCase().includes(q))
			: modules;
	});
	const repeated = $derived(repeatedNames(modules));

	const options = $derived(view ? optionFields(view.options) : []);
	const enabled = $derived(draft?.get('enabled') === true);

	/**
	 * Tasks and threads fall back to the module's limit while the override is 0
	 * (baseunits/WebsiteModules.pas:398-412). The connection override has no such state: while
	 * overrides are on it replaces the module's limit, 0 meaning unlimited
	 * (baseunits/WebsiteModulesSettings.pas:126-155).
	 */
	const LIMITS = [
		{ key: 'max_task_limit', label: 'Max downloads at once', fallsBack: true },
		{ key: 'max_thread_per_task_limit', label: 'Threads per download', fallsBack: true },
		{ key: 'max_connection_limit', label: 'Max connections', fallsBack: false }
	] as const;
	type LimitKey = (typeof LIMITS)[number]['key'];

	const usesDefault = (key: LimitKey): boolean => draft?.get(`limits.${key}`) === 0;
	function setDefault(key: LimitKey, on: boolean) {
		if (!draft || !view) return;
		// Turning the default off starts from the module's own limit.
		draft.set(`limits.${key}`, on ? 0 : view.module_limits[key] || 1);
	}
	const limitText = (n: number | undefined) => (n ? String(n) : 'unlimited');

	const HTTP_FIELDS: Field[] = [
		{
			path: 'http.user_agent',
			label: 'User agent',
			help: 'Leave empty for the global one.',
			control: { kind: 'text' }
		},
		{
			path: 'http.cookies',
			label: 'Cookies',
			help: 'Sent with every request, e.g. name=value; other=value.',
			control: { kind: 'text' }
		},
		{
			path: 'http.proxy.type',
			label: 'Proxy',
			control: {
				kind: 'select',
				choices: [
					{ value: 'default', label: 'Global proxy settings' },
					{ value: 'direct', label: 'No proxy' },
					{ value: 'http', label: 'HTTP' },
					{ value: 'socks4', label: 'SOCKS4' },
					{ value: 'socks5', label: 'SOCKS5' }
				]
			}
		},
		{ path: 'http.proxy.host', label: 'Proxy host', control: { kind: 'text' } },
		{ path: 'http.proxy.port', label: 'Proxy port', control: { kind: 'text' } },
		{ path: 'http.proxy.username', label: 'Proxy username', control: { kind: 'text' } },
		{
			path: 'http.proxy.password',
			label: 'Proxy password',
			control: { kind: 'text', secret: true }
		}
	];
</script>

<div class="modules">
	<div class="picker">
		<input
			class="input"
			type="search"
			placeholder="Search modules"
			aria-label="Search modules"
			bind:value={query}
		/>
		<ul class="list" aria-label="Modules">
			{#each matches as m (moduleKey(m))}
				<li>
					<button
						type="button"
						class="pick"
						aria-pressed={m.id === selected}
						onclick={() => onselect(m.id)}
					>
						<span>
							{m.name}
							{#if repeated.has(m.name)}
								<span class="host small muted">{moduleHost(m)}</span>
							{/if}
						</span>
						{#if m.option_count}
							<span class="count small muted"
								>{m.option_count}
								{m.option_count === 1 ? 'option' : 'options'}</span
							>
						{/if}
					</button>
				</li>
			{:else}
				<li class="small muted empty">
					{modules.length ? 'No module matches.' : 'No modules are loaded.'}
				</li>
			{/each}
		</ul>
	</div>

	<div class="detail">
		{#if !selected}
			<p class="muted">Pick a module to edit its options and limits.</p>
		{:else if loading || !view || !draft}
			<p class="muted">Loading…</p>
		{:else}
			<h3>{view.name}</h3>

			<fieldset>
				<legend class="label">Options</legend>
				{#each options as field (field.path)}
					<SettingField {field} {draft} idPrefix="module" />
				{:else}
					<p class="small muted">This module declares no options.</p>
				{/each}
			</fieldset>

			<SettingField
				field={{
					path: 'enabled',
					label: 'Override limits and connection settings',
					help: 'The settings below apply only while this is on.',
					control: { kind: 'checkbox' }
				}}
				{draft}
				idPrefix="module"
			/>

			<fieldset disabled={!enabled}>
				<legend class="label">Limits</legend>
				{#each LIMITS as limit (limit.key)}
					{@const path = `limits.${limit.key}`}
					{@const declared = limitText(view.module_limits[limit.key])}
					<div class="limit">
						<SettingField
							field={{
								path,
								label: limit.label,
								help: limit.fallsBack
									? undefined
									: `Replaces the module's limit (${declared}); 0 is unlimited.`,
								control: { kind: 'number', min: 0, max: 4294967295 }
							}}
							{draft}
							idPrefix="module"
						/>
						{#if limit.fallsBack}
							<label class="check small">
								<input
									type="checkbox"
									checked={usesDefault(limit.key)}
									onchange={(e) => setDefault(limit.key, e.currentTarget.checked)}
								/>
								Use module default ({declared})
							</label>
						{/if}
					</div>
				{/each}
			</fieldset>

			<fieldset disabled={!enabled}>
				<legend class="label">Connection</legend>
				{#each HTTP_FIELDS as field (field.path)}
					<SettingField {field} {draft} idPrefix="module" />
				{/each}
			</fieldset>
		{/if}
	</div>
</div>

<style>
	.modules {
		display: flex;
		gap: var(--sp-4);
		align-items: flex-start;
	}
	.picker {
		flex: 0 0 240px;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		max-height: 360px;
		overflow-y: auto;
		border: 1px solid var(--line);
		border-radius: var(--r);
		background: var(--surface);
	}
	.pick {
		width: 100%;
		display: flex;
		justify-content: space-between;
		gap: var(--sp-2);
		border: 0;
		background: transparent;
		padding: var(--sp-2) var(--sp-3);
		text-align: left;
		color: var(--fg);
	}
	.host {
		font-weight: 400;
	}
	.pick:hover {
		background: var(--surface-2);
	}
	.pick[aria-pressed='true'] {
		background: var(--accent-soft);
		color: var(--accent);
		font-weight: 600;
	}
	.empty {
		padding: var(--sp-2) var(--sp-3);
	}
	.detail {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	fieldset {
		border: 0;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
	}
	fieldset:disabled {
		opacity: 0.55;
	}
	legend {
		margin-bottom: var(--sp-1);
	}
	.limit .check {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		padding-left: var(--sp-3);
	}
	@media (max-width: 860px) {
		.modules {
			flex-direction: column;
			align-items: stretch;
		}
		.picker {
			flex-basis: auto;
		}
		.list {
			max-height: 220px;
		}
	}
</style>
