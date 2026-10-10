<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, events, session } from '#lib/app.ts';
	import { ValidationError } from '#lib/api/client.ts';
	import { adoptAppearance, previewAppearance } from '#lib/appearance.ts';
	import type {
		ModuleSettingsView,
		ModuleSummary,
		RenamePreview,
		RenamePreviewRequest,
		Settings
	} from '#lib/api/types.ts';
	import AccountsPanel from '#lib/components/settings/AccountsPanel.svelte';
	import DestinationsEditor from '#lib/components/settings/DestinationsEditor.svelte';
	import MangaBakaPanel from '#lib/components/settings/MangaBakaPanel.svelte';
	import ModuleSettings from '#lib/components/settings/ModuleSettings.svelte';
	import SettingField from '#lib/components/settings/SettingField.svelte';
	import WebsiteSelection from '#lib/components/settings/WebsiteSelection.svelte';
	import { Draft } from '#lib/settings/draft.svelte.ts';
	import { editable } from '#lib/settings/module.ts';
	import { showFieldErrors } from '#lib/settings/save.ts';
	import {
		OWN_SECTION_PATHS,
		SECTION_EXTRA_PATHS,
		SETTINGS_SECTIONS
	} from '#lib/settings/sections.ts';

	const TOC = [
		...SETTINGS_SECTIONS.map(({ id, title }) => ({ id, title })),
		{ id: 'websites', title: 'Websites' },
		{ id: 'modules', title: 'Website modules' },
		{ id: 'accounts', title: 'Accounts' }
	];

	let draft = $state<Draft<Settings> | null>(null);
	let loadError = $state<string | null>(null);

	let modules = $state<ModuleSummary[]>([]);
	let moduleView = $state<ModuleSettingsView | null>(null);
	let moduleDraft = $state<Draft<object> | null>(null);
	let moduleLoading = $state(false);
	const selected = $derived(page.url.searchParams.get('module'));

	let saving = $state(false);
	let saveError = $state<string | null>(null);
	let saved = $state(false);
	const dirty = $derived(!!draft?.dirty || !!moduleDraft?.dirty);
	/** An error is cleared by editing its field; until then there is nothing valid to save. */
	const hasModuleErrors = $derived(Object.keys(moduleDraft?.errors ?? {}).length > 0);
	const hasErrors = $derived(Object.keys(draft?.errors ?? {}).length > 0 || hasModuleErrors);

	/**
	 * The section on show, from the URL's `#section-<id>`: a link opens it and Back/Forward move
	 * between sections. Without one, `?module=` opens the module settings, else the first section.
	 */
	const active = $derived.by(() => {
		const id = page.url.hash.replace(/^#section-/, '');
		if (TOC.some((entry) => entry.id === id)) return id;
		return selected ? 'modules' : (TOC[0]?.id ?? '');
	});
	/** What a section holds that is out of view while another one shows: an error or an edit. */
	type Pending = 'invalid' | 'dirty';
	const PENDING_LABEL: Record<Pending, string> = {
		invalid: 'Invalid settings',
		dirty: 'Unsaved changes'
	};
	function pending(id: string): Pending | null {
		const fields =
			id === 'websites'
				? OWN_SECTION_PATHS
				: SETTINGS_SECTIONS.find((s) => s.id === id)?.fields.map((f) => f.path);
		if (fields) {
			const paths = [...fields, ...(SECTION_EXTRA_PATHS[id] ?? [])];
			const errors = Object.keys(draft?.errors ?? {});
			const invalid = (p: string) => errors.some((e) => e === p || e.startsWith(`${p}.`));
			if (paths.some(invalid)) return 'invalid';
			return paths.some((p) => draft?.isDirty(p)) ? 'dirty' : null;
		}
		if (id !== 'modules') return null;
		if (hasModuleErrors) return 'invalid';
		return moduleDraft?.dirty ? 'dirty' : null;
	}

	const section = $derived(SETTINGS_SECTIONS.find((s) => s.id === active));
	/** The folder the server loads the user's `custom.css` from, once `/api/about` says. */
	let dataDir = $state<string | null>(null);

	// An edited appearance shows at once, before saving; discarding it, or leaving the page with it
	// unsaved, shows the saved one again.
	$effect(() => {
		const appearance = (draft?.value as Settings | undefined)?.appearance;
		if (appearance) previewAppearance($state.snapshot(appearance));
	});
	$effect(() => () => previewAppearance(null));
	let preview = $state<RenamePreview | null>(null);

	$effect(() => {
		api
			.getSettings()
			.then((settings) => {
				adoptAppearance(settings.appearance);
				draft = new Draft(settings);
			})
			.catch(() => (loadError = 'Could not load the settings.'));
		api
			.listModules()
			.then((list) => (modules = list))
			.catch(() => (modules = []));
		api
			.about()
			.then(({ data_dir }) => (dataDir = data_dir ?? null))
			.catch(() => (dataDir = null));
	});

	// The previous module stays on show until the next one is loaded, so the panel doesn't collapse
	// to "Loading…" and back, changing the page's height under the reader.
	$effect(() => {
		const id = selected;
		if (!id) {
			moduleView = null;
			moduleDraft = null;
			moduleLoading = false;
			return;
		}
		moduleLoading = true;
		/** Whether `id` is still the one to show, not overtaken by a later pick. */
		const stillSelected = () => page.url.searchParams.get('module') === id;
		api
			.getModuleSettings(id)
			.then((view) => {
				if (!stillSelected()) return;
				moduleView = view;
				moduleDraft = new Draft(editable(view));
			})
			.catch(() => {
				if (!stillSelected()) return;
				moduleView = null;
				moduleDraft = null;
				saveError = `Could not load the settings of module ${id}.`;
			})
			.finally(() => {
				if (stillSelected()) moduleLoading = false;
			});
	});

	function selectModule(id: string) {
		if (id === selected) return;
		if (moduleDraft?.dirty) {
			if (!confirm(`Discard the unsaved changes to ${moduleView?.name}?`)) return;
			// The module stays on show until the next one loads; its dropped edits mustn't be saved.
			moduleDraft.reset();
		}
		const url = new URL(page.url.href);
		url.searchParams.set('module', id);
		goto(url, { replace: true, reset: false });
	}

	// Preview the naming settings as they are typed, debounced.
	$effect(() => {
		if (!draft) return;
		const { saveto, images, output } = $state.snapshot(draft.value) as Settings;
		const request: RenamePreviewRequest = { saveto, images, output };
		const timer = setTimeout(() => {
			api
				.previewRename(request)
				.then((p) => (preview = p))
				.catch(() => (preview = null));
		}, 250);
		return () => clearTimeout(timer);
	});

	/** Where a download's first page ends up, inside its archive when chapters are packed. */
	const previewPath = $derived.by(() => {
		if (!preview) return '';
		return preview.path.endsWith(preview.page) ? preview.path : `${preview.path} › ${preview.page}`;
	});

	/** Saves the settings and the open module's settings together: both or neither. */
	async function save() {
		const changes = draft?.changes();
		const moduleChanges = moduleView ? moduleDraft?.changes() : null;
		const moduleId = moduleView?.id;
		saving = true;
		saveError = null;
		try {
			const result = await api.patchAllSettings({
				...(changes ? { settings: changes } : {}),
				...(moduleId && moduleChanges ? { modules: { [moduleId]: moduleChanges } } : {})
			});
			draft?.commit(result.settings);
			adoptAppearance(result.settings.appearance);
			const view = moduleId ? result.modules[moduleId] : undefined;
			if (view && moduleDraft) {
				moduleView = view;
				moduleDraft.commit(editable(view));
				// The list marks the modules whose settings differ from the defaults.
				api
					.listModules()
					.then((list) => (modules = list))
					// The settings are saved; only the markers may stay stale until a reload.
					.catch(() => {});
			}
		} catch (e) {
			await reject(e, moduleId);
			return;
		} finally {
			saving = false;
		}
		saved = true;
		setTimeout(() => (saved = false), 2000);
		// The password setting decides whether the server warns that it is open.
		if (changes && 'server' in changes) session.checkHealth(api).catch(() => {});
	}

	/** Shows every invalid field inline and scrolls to the first. */
	async function reject(e: unknown, moduleId: string | undefined) {
		if (!(e instanceof ValidationError)) {
			saveError = 'Could not save the settings.';
			return;
		}
		const module = moduleId && moduleDraft ? { id: moduleId, draft: moduleDraft } : null;
		const unplaced = showFieldErrors(e.fields, draft, module);
		if (unplaced.length || !e.fields.length) saveError = unplaced.join('; ') || e.detail;
		// A hidden section's fields are not in the page: show the first one with an error.
		const invalid = TOC.find((entry) => pending(entry.id) === 'invalid');
		if (invalid) await show(invalid.id);
		await tick();
		const first = document.querySelector<HTMLElement>('.field.invalid');
		if (first) {
			first.scrollIntoView({ block: 'center' });
			const control =
				first.querySelector<HTMLElement>('[aria-invalid="true"]') ??
				first.querySelector<HTMLElement>('input, select');
			control?.focus({ preventScroll: true });
		}
	}

	function discard() {
		draft?.reset();
		moduleDraft?.reset();
		saveError = null;
	}

	/** Shows section `id` alone; it keeps the unsaved edits of every section. */
	async function show(id: string) {
		if (id === active) return;
		const { pathname, search } = page.url;
		await goto(`${pathname}${search}#section-${id}`, { reset: false });
		// A long section may have been scrolled; the new one starts at its top.
		if (window.scrollY > 0) window.scrollTo({ top: 0 });
	}

	/** Leaves a click that opens a new tab or window to the browser. */
	function onTocClick(e: MouseEvent, id: string) {
		if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
		e.preventDefault();
		show(id);
	}

	function onBeforeUnload(event: BeforeUnloadEvent) {
		if (dirty) event.preventDefault();
	}

	/** Opens the setup at its first step; it doesn't redirect other pages, as it was finished. */
	async function runSetupAgain() {
		// Where a run left off before is stale now.
		await api.patchSettings({ general: { setup_step: '' } }).catch(() => {});
		await goto('/setup');
	}
</script>

<svelte:head><title>Settings · FMD2r</title></svelte:head>
<svelte:window onbeforeunload={onBeforeUnload} />

<div class="page settings">
	<h1>Settings</h1>

	{#if loadError}
		<p class="error" role="alert">{loadError}</p>
	{:else if !draft}
		<p class="muted">Loading settings…</p>
	{:else}
		<div class="layout">
			<nav class="toc" aria-label="Settings sections">
				<ul class="toc-list">
					{#each TOC as entry (entry.id)}
						{@const mark = pending(entry.id)}
						<li>
							<a
								href="#section-{entry.id}"
								class:active={active === entry.id}
								aria-current={active === entry.id ? 'location' : undefined}
								class:invalid={mark === 'invalid'}
								title={mark ? PENDING_LABEL[mark] : undefined}
								onclick={(e) => onTocClick(e, entry.id)}
								>{entry.title}{#if mark}<span class="mark" aria-hidden="true"></span>{/if}</a
							>
						</li>
					{/each}
				</ul>
				<select
					class="toc-select input"
					aria-label="Jump to section"
					value={active}
					onchange={(e) => show(e.currentTarget.value)}
				>
					{#each TOC as entry (entry.id)}
						<option value={entry.id}>{entry.title}</option>
					{/each}
				</select>
			</nav>

			<div class="sections">
				{#if active === 'modules'}
					<section id="section-modules" class="card" aria-labelledby="heading-modules">
						<h2 id="heading-modules">Website modules</h2>
						<ModuleSettings
							{modules}
							{selected}
							view={moduleView}
							draft={moduleDraft}
							loading={moduleLoading}
							destinations={(draft.value as Settings).saveto.destinations}
							onselect={selectModule}
						/>
					</section>
				{:else if active === 'websites'}
					<section id="section-websites" class="card" aria-labelledby="heading-websites">
						<h2 id="heading-websites">Websites</h2>
						<WebsiteSelection {modules} {draft} />
					</section>
				{:else if active === 'accounts'}
					<section id="section-accounts" class="card" aria-labelledby="heading-accounts">
						<h2 id="heading-accounts">Accounts</h2>
						<p class="small muted">
							Logins for websites that support them. Changes save right away; passwords are stored
							encrypted and never shown again.
						</p>
						<AccountsPanel {api} {modules} />
					</section>
				{:else if section}
					<section id="section-{section.id}" class="card" aria-labelledby="heading-{section.id}">
						<h2 id="heading-{section.id}">{section.title}</h2>
						{#if section.id === 'saveto'}
							<DestinationsEditor {draft} checkFolders={(paths) => api.checkFolders(paths)} />
						{/if}
						{#each section.fields as field (field.path)}
							<SettingField {field} {draft} overridden={session.overridden(field.path)} />
						{/each}
						{#if section.id === 'metadata'}
							<MangaBakaPanel {api} store={events} />
						{/if}
						{#if section.id === 'general'}
							<p class="setup-again">
								<button class="btn" type="button" disabled={dirty} onclick={runSetupAgain}>
									Run setup again
								</button>
								<span class="small muted">
									{dirty
										? 'Save or discard your changes first.'
										: 'Goes through the setup with the current values.'}
								</span>
							</p>
							<p class="custom-css small muted">
								Your own styles: a <span class="mono">custom.css</span>
								in the data folder{#if dataDir}{' '}(<span class="mono">{dataDir}</span>){/if}
								is loaded after the app's, on the next reload.
							</p>
						{/if}
						{#if section.id === 'saveto'}
							<p class="preview small" aria-live="polite">
								<span class="label">Preview</span>
								<span class="mono">{previewPath || '…'}</span>
							</p>
						{/if}
					</section>
				{/if}
			</div>
		</div>
	{/if}

	{#if dirty || hasErrors || saveError || saved}
		<div class="save-bar" role="region" aria-label="Save changes">
			<span class="status small" role="status">
				{#if saveError}
					<span class="error">{saveError}</span>
				{:else if hasErrors}
					Fix the highlighted settings to save
				{:else if dirty}
					Unsaved changes
				{:else}
					Saved
				{/if}
			</span>
			{#if dirty || hasErrors}
				<button class="btn" type="button" disabled={saving} onclick={discard}>Discard</button>
				<button class="btn primary" type="button" disabled={saving || hasErrors} onclick={save}>
					{saving ? 'Saving…' : 'Save'}
				</button>
			{/if}
		</div>
	{/if}
</div>

<style>
	/* Room to scroll the last field above the save bar. */
	.settings {
		padding-bottom: 80px;
	}
	.layout {
		display: flex;
		gap: var(--sp-5);
		align-items: flex-start;
	}
	.toc {
		flex: 0 0 190px;
		position: sticky;
		top: 72px;
	}
	.toc-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.toc-list a {
		display: block;
		padding: var(--sp-1) var(--sp-3);
		border-left: 2px solid var(--line);
		color: var(--muted);
		text-decoration: none;
	}
	/* A dot for a section with edits or errors that are out of view. */
	.mark {
		display: inline-block;
		width: 6px;
		height: 6px;
		margin-left: var(--sp-2);
		vertical-align: middle;
		border-radius: 50%;
		background: var(--accent);
	}
	.toc-list a.invalid .mark {
		background: var(--bad);
	}
	.toc-list a:hover {
		color: var(--fg);
	}
	.toc-list a.active {
		color: var(--accent);
		border-left-color: var(--accent);
		font-weight: 600;
	}
	.toc-select {
		display: none;
	}
	.sections {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: var(--sp-4);
	}
	.card {
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		padding: var(--sp-3) var(--sp-4);
		display: flex;
		flex-direction: column;
		scroll-margin-top: 80px;
	}
	h2 {
		font-size: var(--fs-lg);
		margin-bottom: var(--sp-2);
	}
	.preview {
		display: flex;
		flex-wrap: wrap;
		gap: var(--sp-2);
		align-items: baseline;
		margin: var(--sp-2) 0 0 var(--sp-3);
		word-break: break-all;
	}
	/* Where the queue dock sits on other pages. */
	.save-bar {
		position: fixed;
		left: var(--sp-2);
		right: var(--sp-2);
		bottom: calc(var(--bottom-nav-h) + var(--safe-bottom) + var(--sp-2));
		z-index: 25;
		max-width: 900px;
		margin: 0 auto;
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-xl);
		box-shadow: var(--shadow);
		padding: var(--sp-2) var(--sp-3);
	}
	.status {
		flex: 1;
	}
	.error {
		color: var(--bad);
	}

	/* Phones: the table of contents collapses to a dropdown that stays at the top. */
	@media (max-width: 860px) {
		.layout {
			flex-direction: column;
			align-items: stretch;
			gap: var(--sp-3);
		}
		.toc {
			flex-basis: auto;
			/* Below the sticky top bar. */
			top: calc(var(--safe-top) + 53px);
			z-index: 10;
			background: var(--bg);
			padding: var(--sp-2) 0;
		}
		.toc-list {
			display: none;
		}
		.toc-select {
			display: block;
			width: 100%;
		}
		.card {
			scroll-margin-top: 120px;
		}
	}
	@media (min-width: 861px) {
		.save-bar {
			left: var(--sp-4);
			right: var(--sp-4);
			bottom: calc(var(--sp-4) + var(--safe-bottom));
			padding: 10px 14px;
		}
	}
	.setup-again {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--sp-2);
		margin: var(--sp-3) 0 0;
	}
	.custom-css {
		margin: var(--sp-3) 0 0;
		overflow-wrap: anywhere;
	}
</style>
