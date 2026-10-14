<script lang="ts">
	import { onMount } from 'svelte';
	import type { RelocationCounts } from '#lib/api/types.ts';
	import type { RelocateAnswer } from '#lib/destinations/destinations.ts';

	let {
		used,
		onanswer
	}: {
		/** The changed destinations that library series or websites download to. */
		used: RelocationCounts[];
		/**
		 * `repoint` points them to the new folders, `moveFiles` also moving the chapters already
		 * downloaded; `null` when closed without an answer, which saves nothing.
		 */
		onanswer: (answer: RelocateAnswer | null) => void;
	} = $props();

	const uid = $props.id();
	let dialog: HTMLDialogElement | undefined = $state();
	let moveFiles = $state(false);
	let answered = false;

	const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

	onMount(() => dialog?.showModal());

	function answer(repoint: boolean) {
		answered = true;
		onanswer({ repoint, moveFiles: repoint && moveFiles });
		dialog?.close();
	}
</script>

<dialog
	class="dialog"
	bind:this={dialog}
	aria-labelledby="{uid}-title"
	onclose={() => {
		if (!answered) onanswer(null);
	}}
>
	<h2 id="{uid}-title">Repoint the library</h2>
	<ul class="changes">
		{#each used as change, i (i)}
			<li class="small">
				{change.series} library series and {plural(change.websites, 'website', 'websites')} use
				<span class="mono">{change.from}</span>. Point them to
				<span class="mono">{change.to}</span>?
			</li>
		{/each}
	</ul>
	<label class="check">
		<input type="checkbox" bind:checked={moveFiles} />
		Also move their chapters already downloaded
	</label>
	{#if moveFiles}
		<p class="small muted">
			Each series' chapters move to its new folder in the background; the inbox reports the result.
			A series that downloads straight into the destination's folder is repointed but not moved, as
			that would move the whole folder.
		</p>
	{/if}
	<div class="actions">
		<button class="btn primary" type="button" onclick={() => answer(true)}>Repoint</button>
		<button class="btn" type="button" onclick={() => answer(false)}>Leave them</button>
	</div>
</dialog>

<style>
	.dialog {
		width: min(520px, calc(100vw - 32px));
		border: 1px solid var(--line);
		border-radius: var(--r-lg);
		background: var(--surface);
		color: var(--fg);
		box-shadow: var(--shadow);
		padding: var(--sp-4);
	}
	.dialog[open] {
		display: flex;
		flex-direction: column;
		gap: var(--sp-3);
	}
	.dialog::backdrop {
		background: var(--scrim);
	}
	h2 {
		margin: 0;
		font-size: var(--fs-lg);
	}
	p {
		margin: 0;
	}
	.changes {
		margin: 0;
		padding-left: var(--sp-4);
		display: flex;
		flex-direction: column;
		gap: var(--sp-1);
	}
	.mono {
		word-break: break-all;
	}
	.check {
		display: flex;
		align-items: center;
		gap: var(--sp-2);
	}
	.actions {
		display: flex;
		gap: var(--sp-2);
	}
</style>
