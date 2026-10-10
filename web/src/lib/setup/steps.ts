import type { Component } from 'svelte';
import type { Api, MergePatch } from '#lib/api/client.ts';
import type { Health, Settings } from '#lib/api/types.ts';
import type { EventStore } from '#lib/events.svelte.ts';

/** What the wizard gives a step's component. */
export interface StepProps {
	api: Api;
	/** The server's live events, e.g. a download's progress. */
	store: EventStore;
	/** The settings as saved so far, including the earlier steps' choices. */
	settings: Settings;
	/** Saves the step and finishes the setup, then opens `to`, e.g. a Settings section. */
	finish: (to: string) => Promise<void>;
}

/** What a step's component may export. */
export interface StepExports {
	/** Whether Next is allowed yet (always, without it). Read reactively. */
	ready?: () => boolean;
	/** What Next reads instead, e.g. "Skip" while nothing was entered. Read reactively. */
	nextLabel?: () => string | undefined;
	/**
	 * The settings the step chose, as a merge patch the wizard saves with `PATCH /api/settings`
	 * (nothing to save when it returns nothing). Run on Next; rejecting keeps the user on the step.
	 */
	save?: () => Promise<MergePatch | void>;
}

/** One step of the setup wizard. */
export interface SetupStep {
	/** Stored as `general.setup_step` to resume at after a reload. */
	id: string;
	title: string;
	component: Component<StepProps, StepExports>;
	/** Whether the step applies to this server (always, without it). Asked once, as setup opens. */
	shows?: (health: Health) => boolean;
}
