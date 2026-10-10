<script lang="ts">
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, settings }: StepProps = $props();

	/** Set by this step already, when the user came Back to it. */
	const isSet = $derived(settings.server.has_auth_token);

	let password = $state('');
	let confirmation = $state('');

	/** Both empty skips the step; otherwise they must match. */
	export function ready(): boolean {
		return password === confirmation;
	}

	export function nextLabel(): string | undefined {
		return password === '' && confirmation === '' && !isSet ? 'Skip' : undefined;
	}

	/**
	 * Saves the password itself, which ends every session (this one too), then logs in with it so
	 * the wizard can save its progress instead of landing on the login screen.
	 */
	export async function save(): Promise<void> {
		if (password === '') return;
		if (!(await api.changePassword(password)))
			throw new Error('Could not log in with the new password.');
	}
</script>

{#if isSet}
	<p>A password is set now. Enter a new one to change it, or go on to the finish.</p>
{:else}
	<p>
		This server is reachable from other machines and no password is set: anyone who can reach it can
		use it. Set a password to keep them out.
	</p>
{/if}
<label class="field">
	<span class="label">Password</span>
	<input class="input" type="password" autocomplete="new-password" bind:value={password} />
</label>
<label class="field">
	<span class="label">Confirm password</span>
	<input class="input" type="password" autocomplete="new-password" bind:value={confirmation} />
</label>
{#if confirmation !== '' && password !== confirmation}
	<p class="small mismatch">The passwords don't match.</p>
{/if}
{#if !isSet}
	<p class="small muted">Skip leaves the server open; Settings can set a password later.</p>
{/if}

<style>
	p {
		margin: 0;
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.mismatch {
		color: var(--bad);
	}
</style>
