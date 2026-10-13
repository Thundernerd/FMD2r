<script lang="ts">
	import { ApiError } from '#lib/api/client.ts';
	import type { StepProps } from '#lib/setup/steps.ts';

	let { api, settings }: StepProps = $props();

	/** Whether the restore path is open. */
	let restoring = $state(false);
	let file = $state<File | null>(null);
	let uploading = $state(false);
	let error = $state<string | null>(null);
	/** The backup is staged: it replaces this server's data at the next start. */
	let restored = $state(false);

	export function busy(): boolean {
		return uploading;
	}

	export function ended(): boolean {
		return restored;
	}

	function message(e: unknown): string {
		if (!(e instanceof ApiError)) return 'Could not reach the server.';
		if (e.status === 413) return e.detail ?? 'The zip is too large.';
		return e.detail ?? 'The restore failed.';
	}

	async function restore() {
		if (!file) return;
		uploading = true;
		error = null;
		try {
			await api.restoreBackup(file);
			restored = true;
		} catch (e) {
			error = message(e);
		} finally {
			uploading = false;
		}
	}
</script>

{#if restored}
	<p role="status">The backup is valid. Restart FMD2r to finish restoring.</p>
	<p class="small muted">
		At its next start FMD2r replaces this install's data with the backup's library, settings and
		saved logins. The website lists and MangaBaka data download again.
	</p>
{:else}
	<p>
		FMD2r downloads manga from hundreds of websites, keeps a library of the series you follow and
		downloads their new chapters. It runs the website modules of Free Manga Downloader 2 (FMD2).
	</p>
	<p>A few questions get it ready:</p>
	<ul class="covers">
		<li>where downloads are saved, and in which format;</li>
		<li>which websites to browse in Discover;</li>
		<li>the optional MangaBaka database, for better series details.</li>
	</ul>
	<p class="small muted">
		{#if settings.general.setup_completed}
			Each step starts from your current settings; change what you like.
		{:else}
			Each choice is saved as you go, and can be changed later in Settings.
		{/if}
	</p>

	{#if !settings.general.setup_completed}
		<div class="restore">
			<p>Moving FMD2r? Restore the backup you downloaded with “Download backup” in its settings.</p>
			{#if restoring}
				<label class="field">
					<span class="label">FMD2r backup, zipped</span>
					<input
						class="input"
						type="file"
						accept=".zip,application/zip"
						onchange={(e) => (file = e.currentTarget.files?.[0] ?? null)}
					/>
				</label>
				<div>
					<button class="btn primary" type="button" disabled={!file || uploading} onclick={restore}
						>{uploading ? 'Restoring…' : 'Restore'}</button
					>
				</div>
				{#if error}
					<p class="bad" role="alert">{error}</p>
				{/if}
			{:else}
				<div>
					<button class="btn" type="button" onclick={() => (restoring = true)}>
						Restore from a backup
					</button>
				</div>
			{/if}
		</div>
	{/if}
{/if}

<style>
	p,
	ul {
		margin: 0;
	}
	.covers {
		padding-left: var(--sp-5);
	}
	.restore {
		display: flex;
		flex-direction: column;
		gap: var(--sp-2);
		padding-top: var(--sp-3);
		border-top: 1px solid var(--line);
	}
	.bad {
		color: var(--bad);
	}
</style>
