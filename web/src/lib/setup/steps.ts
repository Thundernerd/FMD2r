import type { Component } from 'svelte';
import type { Api, MergePatch } from '#lib/api/client.ts';
import type { Settings } from '#lib/api/types.ts';

/** What the wizard gives a step's component. */
export interface StepProps {
	api: Api;
	/** The settings as saved so far, including the earlier steps' choices. */
	settings: Settings;
}

/** What a step's component may export. */
export interface StepExports {
	/** Whether Next is allowed yet (always, without it). Read reactively. */
	ready?: () => boolean;
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
}
