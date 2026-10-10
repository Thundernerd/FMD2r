import DownloadFoldersStep from '#lib/components/setup/DownloadFoldersStep.svelte';
import FinishStep from '#lib/components/setup/FinishStep.svelte';
import ImportStep from '#lib/components/setup/ImportStep.svelte';
import FormatStep from '#lib/components/setup/FormatStep.svelte';
import MangaBakaStep from '#lib/components/setup/MangaBakaStep.svelte';
import PasswordStep from '#lib/components/setup/PasswordStep.svelte';
import WebsitesStep from '#lib/components/setup/WebsitesStep.svelte';
import WelcomeStep from '#lib/components/setup/WelcomeStep.svelte';
import type { Health } from '#lib/api/types.ts';
import { isOpenServer } from '#lib/session.svelte.ts';
import type { SetupStep } from './steps.ts';

/** The server is open, and the password isn't the command line's or environment's to set. */
const needsPassword = (health: Health): boolean =>
	isOpenServer(health) && !health.overridden.includes('server.auth_token');

/**
 * The setup wizard's steps, in order. Each step ticket adds its entry here: after the welcome,
 * import from FMD2 (T84), download folders (T79), download format (T80), MangaBaka (T81),
 * websites (T82) and password (T83, only when the server is open), then the finish. A step that
 * edits settings an FMD2 import brings lists them in `paths`, so it says when it starts from them.
 */
export const SETUP_STEPS: SetupStep[] = [
	{ id: 'welcome', title: 'Welcome', component: WelcomeStep },
	{ id: 'import', title: 'Import from FMD2', component: ImportStep },
	{
		id: 'download-folders',
		title: 'Download folders',
		component: DownloadFoldersStep,
		// The import sets `default_dir`, which moves the default destination.
		paths: ['saveto.default_dir', 'saveto.destinations']
	},
	{ id: 'format', title: 'Download format', component: FormatStep, paths: ['output.format'] },
	{ id: 'mangabaka', title: 'Metadata', component: MangaBakaStep },
	{
		id: 'websites',
		title: 'Websites',
		component: WebsitesStep,
		paths: ['general.selected_websites']
	},
	// Last before the finish: setting a password ends every session, this one included.
	{ id: 'password', title: 'Password', component: PasswordStep, shows: needsPassword },
	{ id: 'finish', title: 'Finish', component: FinishStep }
];
