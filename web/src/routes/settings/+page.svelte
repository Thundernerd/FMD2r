<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api } from '#lib/app.ts';
	import { ValidationError } from '#lib/api/client.ts';
	import type {
		ModuleSettingsView,
		ModuleSummary,
		RenamePreview,
		RenamePreviewRequest,
		Settings
	} from '#lib/api/types.ts';
	import AccountsPanel from '#lib/components/settings/AccountsPanel.svelte';
	import ModuleSettings from '#lib/components/settings/ModuleSettings.svelte';
	import SettingField from '#lib/components/settings/SettingField.svelte';
	import { Draft } from '#lib/settings/draft.svelte.ts';
	import { showFieldErrors } from '#lib/settings/save.ts';
	import { SETTINGS_SECTIONS } from '#lib/settings/sections.ts';

	const TOC = [
		...SETTINGS_SECTIONS.map(({ id, title }) => ({ id, title })),
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
	const hasErrors = $derived(
		Object.keys(draft?.errors ?? {}).length + Object.keys(moduleDraft?.errors ?? {}).length > 0
	);

	let active = $state(TOC[0]?.id ?? '');
	let preview = $state<RenamePreview | null>(null);

	$effect(() => {
		api
			.getSettings()
			.then((settings) => (draft = new Draft(settings)))
			.catch(() => (loadError = 'Could not load the settings.'));
		api
			.listModules()
			.then((list) => (modules = list))
			.catch(() => (modules = []));
	});

	/** The editable part of a module's settings, shaped like its PATCH body. */
	const editable = (view: ModuleSettingsView) => ({
		enabled: view.enabled,
		limits: view.limits,
		http: view.http,
		options: Object.fromEntries(view.options.map((o) => [o.key, o.value]))
	});

	$effect(() => {
		const id = selected;
		moduleView = null;
		moduleDraft = null;
		if (!id) return;
		moduleLoading = true;
		api
			.getModuleSettings(id)
			.then((view) => {
				if (page.url.searchParams.get('module') !== id) return;
				moduleView = view;
				moduleDraft = new Draft(editable(view));
			})
			.catch(() => (saveError = `Could not load the settings of module ${id}.`))
			.finally(() => (moduleLoading = false));
	});

	function selectModule(id: string) {
		if (id === selected) return;
		if (moduleDraft?.dirty && !confirm(`Discard the unsaved changes to ${moduleView?.name}?`)) {
			return;
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
			const view = moduleId ? result.modules[moduleId] : undefined;
			if (view && moduleDraft) {
				moduleView = view;
				moduleDraft.commit(editable(view));
			}
		} catch (e) {
			await reject(e, moduleId);
			return;
		} finally {
			saving = false;
		}
		saved = true;
		setTimeout(() => (saved = false), 2000);
	}

	/** Shows a rejected save: every invalid field inline, the first one scrolled to. */
	async function reject(e: unknown, moduleId: string | undefined) {
		if (!(e instanceof ValidationError)) {
			saveError = 'Could not save the settings.';
			return;
		}
		const module = moduleId && moduleDraft ? { id: moduleId, draft: moduleDraft } : null;
		const unplaced = showFieldErrors(e.fields, draft, module);
		if (unplaced.length || !e.fields.length) saveError = unplaced.join('; ') || e.detail;
		await tick();
		const first = document.querySelector<HTMLElement>('.field.invalid');
		if (first) {
			first.scrollIntoView({ block: 'center' });
			first.querySelector<HTMLElement>('input, select')?.focus({ preventScroll: true });
		}
	}

	function discard() {
		draft?.reset();
		moduleDraft?.reset();
		saveError = null;
	}

	function jump(id: string) {
		active = id;
		document.getElementById(`section-${id}`)?.scrollIntoView({ block: 'start' });
	}

	// Highlight the section being read in the table of contents.
	$effect(() => {
		if (!draft) return;
		const observer = new IntersectionObserver(
			(entries) => {
				const visible = entries.filter((e) => e.isIntersecting);
				const top = visible.sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
				if (top) active = top.target.id.replace(/^section-/, '');
			},
			{ rootMargin: '-80px 0px -60% 0px' }
		);
		for (const { id } of TOC) {
			const el = document.getElementById(`section-${id}`);
			if (el) observer.observe(el);
		}
		return () => observer.disconnect();
	});

	function onBeforeUnload(event: BeforeUnloadEvent) {
		if (dirty) event.preventDefault();
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
						<li>
							<a
								href="#section-{entry.id}"
								class:active={active === entry.id}
								aria-current={active === entry.id ? 'location' : undefined}
								onclick={(e) => {
									e.preventDefault();
									jump(entry.id);
								}}>{entry.title}</a
							>
						</li>
					{/each}
				</ul>
				<select
					class="toc-select input"
					aria-label="Jump to section"
					value={active}
					onchange={(e) => jump(e.currentTarget.value)}
				>
					{#each TOC as entry (entry.id)}
						<option value={entry.id}>{entry.title}</option>
					{/each}
				</select>
			</nav>

			<div class="sections">
				{#each SETTINGS_SECTIONS as section (section.id)}
					<section id="section-{section.id}" class="card" aria-labelledby="heading-{section.id}">
						<h2 id="heading-{section.id}">{section.title}</h2>
						{#each section.fields as field (field.path)}
							<SettingField {field} {draft} />
						{/each}
						{#if section.id === 'saveto'}
							<p class="preview small" aria-live="polite">
								<span class="label">Preview</span>
								<span class="mono">{previewPath || '…'}</span>
							</p>
						{/if}
					</section>
				{/each}

				<section id="section-modules" class="card" aria-labelledby="heading-modules">
					<h2 id="heading-modules">Website modules</h2>
					<ModuleSettings
						{modules}
						{selected}
						view={moduleView}
						draft={moduleDraft}
						loading={moduleLoading}
						onselect={selectModule}
					/>
				</section>

				<section id="section-accounts" class="card" aria-labelledby="heading-accounts">
					<h2 id="heading-accounts">Accounts</h2>
					<p class="small muted">
						Logins for websites that support them. Changes save right away; passwords are stored
						encrypted and never shown again.
					</p>
					<AccountsPanel />
				</section>
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
</style>
