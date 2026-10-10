<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { AccountInfo, AccountState, ModuleSummary } from '#lib/api/types.ts';
	import { groupModules, moduleLabel, repeatedNames } from '#lib/modules.ts';

	let {
		api,
		modules
	}: {
		api: Api;
		/** The loaded modules, whose names, categories and hosts order the accounts. */
		modules: ModuleSummary[];
	} = $props();

	/** FMD2's account list captions (mangadownloader/forms/frmAccountManager.pas:78-81). */
	const STATUS_TEXT: Record<AccountState, string> = {
		unknown: 'Unknown',
		checking: 'Checking',
		valid: 'OK',
		invalid: 'Invalid'
	};

	let accounts = $state<AccountInfo[] | null>(null);
	let error = $state<string | null>(null);
	/** The module whose credentials are being edited. */
	let editing = $state<string | null>(null);
	let username = $state('');
	let password = $state('');
	let busy = $state<string | null>(null);

	/**
	 * The accounts with their modules' names, categories and root URLs, so they are listed like the
	 * website modules. An account whose module isn't among `modules` (e.g. still loading) is listed
	 * under "Other".
	 */
	const joined = $derived.by(() => {
		const byId = new Map(modules.map((m) => [m.id, m] as const));
		return (accounts ?? []).map((account) => {
			const m = byId.get(account.module);
			return {
				account,
				name: m?.name ?? account.name,
				root_url: m?.root_url ?? '',
				category: m?.category ?? ''
			};
		});
	});
	const repeated = $derived(repeatedNames(joined));
	const groups = $derived(groupModules(joined));

	$effect(() => {
		api
			.listAccounts()
			.then((list) => (accounts = list))
			.catch(() => (error = 'Could not load the accounts.'));
	});

	function replace(info: AccountInfo) {
		accounts = (accounts ?? []).map((a) => (a.module === info.module ? info : a));
	}

	async function run(module: string, what: string, f: () => Promise<void>) {
		busy = module;
		error = null;
		try {
			await f();
		} catch (e) {
			error =
				e instanceof ApiError && e.status === 409
					? `A login for ${module} is already running.`
					: `Could not ${what}.`;
		} finally {
			busy = null;
		}
	}

	function edit(account: AccountInfo) {
		editing = account.module;
		username = account.username;
		password = '';
	}

	const login = (module: string) =>
		run(module, 'log in', async () => {
			const current = accounts?.find((a) => a.module === module);
			if (current) replace({ ...current, status: 'checking' });
			replace(await api.loginAccount(module));
		});

	/** Saves new credentials and, like FMD2's account editor, checks them right away
	 * (mangadownloader/forms/frmAccountManager.pas:271-277). */
	const save = (account: AccountInfo) =>
		run(account.module, 'save the account', async () => {
			const changed = username !== account.username || password !== '';
			replace(
				await api.putAccount(account.module, {
					username,
					...(password !== '' ? { password } : {})
				})
			);
			editing = null;
			if (changed) await login(account.module);
		});

	const toggle = (account: AccountInfo, enabled: boolean) =>
		run(account.module, 'change the account', async () => {
			replace(await api.putAccount(account.module, { enabled }));
		});

	const clear = (account: AccountInfo) =>
		run(account.module, 'clear the account', async () => {
			await api.deleteAccount(account.module);
			replace({
				...account,
				enabled: false,
				username: '',
				has_password: false,
				status: 'unknown'
			});
		});
</script>

<div class="accounts">
	{#if error}
		<p class="error small" role="alert">{error}</p>
	{/if}
	{#if !accounts}
		{#if !error}<p class="muted">Loading…</p>{/if}
	{:else if accounts.length === 0}
		<p class="small muted">No loaded module supports accounts.</p>
	{:else}
		{#each groups as group (group.category)}
			<h3 class="category small muted">{group.category}</h3>
			<ul class="list" aria-label={group.category}>
				{#each group.modules as entry (entry.account.module)}
					{@const account = entry.account}
					<li class="row">
						<div class="head">
							<label class="enabled">
								<input
									type="checkbox"
									checked={account.enabled}
									disabled={busy === account.module}
									onchange={(e) => toggle(account, e.currentTarget.checked)}
								/>
								<span class="name">{moduleLabel(entry, repeated)}</span>
							</label>
							<span class="user small muted">{account.username || 'No username'}</span>
							<span class="chip small status-{account.status}">{STATUS_TEXT[account.status]}</span>
							<div class="actions">
								<button
									class="btn sm"
									type="button"
									disabled={busy === account.module}
									onclick={() => edit(account)}>Edit</button
								>
								<button
									class="btn sm primary"
									type="button"
									disabled={busy === account.module || account.status === 'checking'}
									onclick={() => login(account.module)}>Log in</button
								>
							</div>
						</div>
						{#if editing === account.module}
							<form
								class="form"
								onsubmit={(e) => {
									e.preventDefault();
									save(account);
								}}
							>
								<label class="field">
									<span class="label">Username</span>
									<input class="input" autocomplete="username" bind:value={username} />
								</label>
								<label class="field">
									<span class="label">Password</span>
									<input
										class="input"
										type="password"
										autocomplete="new-password"
										placeholder={account.has_password ? 'Unchanged' : ''}
										bind:value={password}
									/>
								</label>
								<div class="actions">
									<button
										class="btn sm ghost"
										type="button"
										disabled={busy === account.module}
										onclick={() => clear(account)}>Clear account</button
									>
									<button class="btn sm" type="button" onclick={() => (editing = null)}
										>Cancel</button
									>
									<button class="btn sm primary" type="submit" disabled={busy === account.module}
										>Save and log in</button
									>
								</div>
							</form>
						{/if}
					</li>
				{/each}
			</ul>
		{/each}
	{/if}
</div>

<style>
	.accounts {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.category {
		margin: var(--sp-2) 0 0;
		font-weight: 600;
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
	}
	.row {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		padding: var(--sp-2) 0;
		border-bottom: 1px solid var(--line);
	}
	.row:last-child {
		border-bottom: none;
	}
	.head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--sp-2);
	}
	.enabled {
		display: flex;
		align-items: center;
		gap: var(--sp-1);
		font-weight: 600;
	}
	.user {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.chip {
		border-radius: var(--r-pill);
		padding: 1px 8px;
		border: 1px solid var(--line);
		background: var(--surface-2);
	}
	.status-valid {
		background: var(--ok-soft);
		border-color: var(--ok);
		color: var(--ok);
	}
	.status-invalid {
		background: var(--bad-soft);
		border-color: var(--bad);
		color: var(--bad);
	}
	.status-checking {
		background: var(--warn-soft);
		border-color: var(--warn);
		color: var(--warn);
	}
	.actions {
		display: flex;
		gap: var(--sp-1);
		justify-content: flex-end;
	}
	.form {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
</style>
