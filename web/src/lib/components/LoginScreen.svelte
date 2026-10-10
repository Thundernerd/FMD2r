<script lang="ts">
	import type { Api } from '#lib/api/client.ts';

	let { api, onlogin }: { api: Api; onlogin: () => void } = $props();

	let password = $state('');
	let busy = $state(false);
	let error = $state<string | null>(null);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		busy = true;
		error = null;
		try {
			if (await api.login(password)) {
				password = '';
				onlogin();
			} else {
				error = 'Wrong password.';
			}
		} catch {
			error = 'Could not reach FMD2r. Try again.';
		} finally {
			busy = false;
		}
	}
</script>

<div class="screen">
	<form class="card" aria-labelledby="login-title" onsubmit={submit}>
		<p class="brand">FMD2r</p>
		<h2 id="login-title" class="title">Log in</h2>
		<label class="field">
			<span class="label">Password</span>
			<input
				class="input"
				type="password"
				autocomplete="current-password"
				bind:value={password}
				oninput={() => (error = null)}
			/>
		</label>
		{#if error}
			<p class="error" role="alert">{error}</p>
		{/if}
		<button class="btn primary submit" type="submit" disabled={busy}>Log in</button>
	</form>
</div>

<style>
	.screen {
		min-height: 100vh;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: var(--sp-4);
	}
	.card {
		width: 100%;
		max-width: 340px;
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
		padding: var(--sp-5);
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: var(--r-xl);
		box-shadow: var(--shadow);
	}
	.brand {
		margin: 0;
		font: 800 var(--fs-title) var(--f-display);
		letter-spacing: -0.02em;
	}
	.title {
		margin: 0;
		font-size: var(--fs-lg);
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.input {
		padding: 8px 10px;
	}
	.error {
		margin: 0;
		padding: 6px 10px;
		border-radius: var(--r);
		background: var(--bad-soft);
		color: var(--bad);
		font-size: var(--fs-sm);
	}
	.submit {
		justify-content: center;
		padding: 8px 10px;
	}
</style>
