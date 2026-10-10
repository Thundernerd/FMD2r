<script lang="ts">
	import type { Destination, ModuleSettingsView, ModuleSummary } from '#lib/api/types.ts';
	import DestinationPicker from '#lib/components/destinations/DestinationPicker.svelte';
	import { defaultDestination, destinationAt } from '#lib/destinations/destinations.ts';
	import type { Draft } from '#lib/settings/draft.svelte.ts';
	import {
		groupModules,
		matchesSearch,
		moduleHost,
		moduleKey,
		repeatedNames
	} from '#lib/modules.ts';
	import { optionFields, type Field } from '#lib/settings/fields.ts';
	import SettingField from './SettingField.svelte';

	let {
		modules,
		selected,
		view,
		draft,
		loading,
		destinations = [],
		onselect
	}: {
		modules: ModuleSummary[];
		selected: string | null;
		/** The selected module's settings as last saved, once loaded. */
		view: ModuleSettingsView | null;
		/** Edits to `view`, with `options` keyed by option key. */
		draft: Draft<object> | null;
		loading: boolean;
		/** The configured destinations (possibly unsaved), for the website's own. */
		destinations?: Destination[];
		onselect: (id: string) => void;
	} = $props();

	const uid = $props.id();
	let query = $state('');
	/** The modules whose name, category, host or ID contains every word of the search. */
	const matches = $derived(
		modules.filter((m) => matchesSearch(`${m.name} ${m.category} ${moduleHost(m)} ${m.id}`, query))
	);
	const groups = $derived(groupModules(matches));
	const count = $derived(
		matches.length === modules.length
			? `${modules.length} ${modules.length === 1 ? 'module' : 'modules'}`
			: `${matches.length} of ${modules.length}`
	);
	const repeated = $derived(repeatedNames(modules));

	let list: HTMLUListElement | undefined = $state();
	/** Arrow keys, Home and End move the focus from the search box through the modules. */
	function navigate(event: KeyboardEvent) {
		if (!list) return;
		const picks = [...list.querySelectorAll<HTMLButtonElement>('.pick')];
		// The focused module's index; -1 in the search box.
		const at = picks.indexOf(event.target as HTMLButtonElement);
		const next = {
			ArrowDown: at + 1,
			ArrowUp: at - 1,
			Home: 0,
			End: picks.length - 1
		}[event.key];
		// In the search box only ArrowDown leaves it; the other keys keep moving the caret.
		if (next === undefined || (at < 0 && event.key !== 'ArrowDown')) return;
		const target = picks[Math.max(0, Math.min(next, picks.length - 1))];
		if (!target) return;
		event.preventDefault();
		target.focus();
	}

	/** The module last brought into view, so searching later doesn't scroll back to it. */
	let shownSelected: string | null = null;
	// Scroll to the selected module once listed, e.g. `?module=` before the modules load.
	$effect(() => {
		void groups; // Re-run when the list changes.
		if (!selected || !list || selected === shownSelected) return;
		const pick = list.querySelector<HTMLElement>('.pick[aria-pressed="true"]');
		if (!pick) return;
		shownSelected = selected;
		// jsdom has no scrollIntoView.
		pick.scrollIntoView?.({ block: 'nearest' });
	});

	/** Whether the panel shows a module while the selected one's settings load. */
	const busy = $derived(loading && !!view && !!draft);

	const options = $derived(view ? optionFields(view.options) : []);
	const websiteDir = $derived(
		typeof draft?.get('save_to') === 'string' ? String(draft.get('save_to')).trim() : ''
	);
	/** A folder of its own that no destination has, e.g. after a destination was edited. */
	const strayDir = $derived(websiteDir !== '' && !destinationAt(destinations, websiteDir));
	const fallback = $derived(defaultDestination(destinations)?.name ?? 'the default destination');
	const enabled = $derived(draft?.get('enabled') === true);

	/**
	 * Tasks and threads fall back to the module's limit while the override is 0
	 * (baseunits/WebsiteModules.pas:398-412); the connection override replaces it, 0 meaning
	 * unlimited (baseunits/WebsiteModulesSettings.pas:126-155).
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
			control: { kind: 'secret' }
		}
	];
</script>

<div class="modules">
	<!-- The keys move the focus between the buttons inside, which take them; the div only
	     listens so one handler serves the search box and every module. -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="picker" onkeydown={navigate}>
		<div class="heading">
			<span class="label">Pick a website to edit its settings</span>
			<span class="small muted" aria-live="polite">{count}</span>
		</div>
		<input
			class="input"
			type="search"
			placeholder="Search by name, category or host"
			aria-label="Search modules"
			bind:value={query}
		/>
		<ul class="list" aria-label="Modules" bind:this={list}>
			{#each groups as group, i (group.category)}
				<li class="group">
					<h4 class="category small muted" id="{uid}-group-{i}">
						{group.category}
					</h4>
					<ul aria-labelledby="{uid}-group-{i}">
						{#each group.modules as m (moduleKey(m))}
							<li>
								<button
									type="button"
									class="pick"
									aria-pressed={m.id === selected}
									onclick={() => {
										// Already in view where it was clicked; scrolling to it could move the page.
										shownSelected = m.id;
										onselect(m.id);
									}}
								>
									<span>
										{m.name}
										{#if repeated.has(m.name)}
											<span class="host small muted">{moduleHost(m)}</span>
										{/if}
										{#if m.customized}
											<span class="custom small" title="Its settings differ from the defaults"
												>Custom</span
											>
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
						{/each}
					</ul>
				</li>
			{:else}
				<li class="small muted empty">
					{modules.length ? 'No module matches.' : 'No modules are loaded.'}
				</li>
			{/each}
		</ul>
	</div>

	<!-- While the next module loads, the previous one stays on show, dimmed and out of reach, so
	     the panel doesn't collapse to "Loading…" and back. -->
	<section class="detail" class:busy aria-label="Module settings" aria-busy={busy} inert={busy}>
		{#if !selected}
			<p class="muted">Pick a module to edit its options and limits.</p>
		{:else if !view || !draft}
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

			<div
				class="field destination"
				class:dirty={draft.isDirty('save_to')}
				class:invalid={!!draft.errors['save_to']}
				data-path="save_to"
			>
				<label class="caption" for="{uid}-save-to">Download destination</label>
				<DestinationPicker
					id="{uid}-save-to"
					label="Download destination"
					inherit="Default destination ({fallback})"
					{destinations}
					bind:value={
						() => (typeof draft?.get('save_to') === 'string' ? String(draft.get('save_to')) : ''),
						(v) => draft?.set('save_to', v)
					}
				/>
				<p class="help small muted">
					Where this website's downloads go unless another folder is picked on the series page.
					Applies whether or not the overrides below are on.
				</p>
				{#if strayDir}
					<p class="help small muted">
						This folder is not one of the destinations (one may have been edited or removed);
						downloads still go there until another is picked.
					</p>
				{/if}
				{#if draft.errors['save_to']}
					<p class="field-error small" role="alert">{draft.errors['save_to']}</p>
				{/if}
			</div>

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
	</section>
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
	.heading {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: var(--sp-2);
	}
	.list,
	.list ul {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.list {
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
	.category {
		position: sticky;
		top: 0;
		margin: 0;
		padding: var(--sp-1) var(--sp-3);
		background: var(--surface-2);
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}
	.custom {
		margin-left: var(--sp-1);
		padding: 0 var(--sp-1);
		border-radius: var(--r);
		background: var(--warn-soft);
		color: var(--warn);
		font-weight: 600;
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
	.detail.busy {
		opacity: 0.55;
		transition: opacity 150ms;
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
	.destination {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
		max-width: 480px;
		padding: var(--sp-2) 0 var(--sp-2) var(--sp-3);
		border-left: 2px solid transparent;
	}
	.destination.dirty {
		border-left-color: var(--accent);
	}
	.destination.invalid {
		border-left-color: var(--bad);
	}
	.destination .caption {
		font-weight: 500;
	}
	.destination .help,
	.destination .field-error {
		margin: 0;
	}
	.destination .field-error {
		color: var(--bad);
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
