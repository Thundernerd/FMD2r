<script lang="ts">
	import { ApiError, type Api } from '#lib/api/client.ts';
	import type { FavoriteView } from '#lib/api/types.ts';
	import { isFailing } from '#lib/library/filters.ts';

	let { api, module, link }: { api: Api; module: string; link: string } = $props();

	/** This series in the library, when its website keeps failing. */
	let failing = $state<FavoriteView | null>(null);
	let checking = $state(false);
	let checkNote = $state<string | null>(null);

	// Loaded apart from the series, so the notice shows when the website can't be reached at all.
	$effect(() => {
		const [m, l] = [module, link];
		failing = null;
		checkNote = null;
		let current = true;
		api
			.listFavorites()
			.then((favorites) => {
				const found = favorites.find((f) => f.module_id === m && f.link === l);
				if (current) failing = found && isFailing(found) ? found : null;
			})
			// Without the library there is nothing to warn about.
			.catch(() => {});
		return () => (current = false);
	});

	/** Checks this series alone, which no failure count holds back. */
	async function checkAgain() {
		if (!failing) return;
		checking = true;
		checkNote = null;
		try {
			await api.checkFavorites([failing.id]);
			checkNote = 'Checking again; new chapters arrive in the inbox.';
		} catch (e) {
			checkNote =
				e instanceof ApiError && e.status === 409
					? 'A check is already running.'
					: e instanceof ApiError && e.status === 503
						? 'The chapter check is not running on this server.'
						: 'Could not start the check.';
		} finally {
			checking = false;
		}
	}
</script>

{#if failing}
	<div class="failing">
		<p role="alert">
			The website hasn't answered for this series in its last {failing.check_failures} checks: {failing.last_check_error ??
				'no reason given'}
		</p>
		<div class="actions">
			<button class="btn sm" type="button" disabled={checking} onclick={checkAgain}
				>Check again</button
			>
			<a class="btn sm" href="/discover?{new URLSearchParams({ q: failing.title })}"
				>Find on another website</a
			>
			{#if checkNote}
				<span class="small" role="status">{checkNote}</span>
			{/if}
		</div>
	</div>
{/if}

<style>
	.failing {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		padding: var(--sp-3) var(--sp-4);
		border-radius: var(--r-lg);
		background: var(--warn-soft);
		color: var(--warn);
		max-width: 65ch;
	}
	p {
		margin: 0;
	}
	.actions {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
		flex-wrap: wrap;
	}
</style>
