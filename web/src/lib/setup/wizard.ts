import DownloadFoldersStep from '#lib/components/setup/DownloadFoldersStep.svelte';
import FinishStep from '#lib/components/setup/FinishStep.svelte';
import FormatStep from '#lib/components/setup/FormatStep.svelte';
import MangaBakaStep from '#lib/components/setup/MangaBakaStep.svelte';
import WebsitesStep from '#lib/components/setup/WebsitesStep.svelte';
import WelcomeStep from '#lib/components/setup/WelcomeStep.svelte';
import type { SetupStep } from './steps.ts';

/**
 * The setup wizard's steps, in order. Each step ticket adds its entry here: after the welcome,
 * import from FMD2 (T84), download folders (T79), download format (T80), MangaBaka (T81),
 * websites (T82) and password (T83, only when the server is open), then the finish.
 */
export const SETUP_STEPS: SetupStep[] = [
	{ id: 'welcome', title: 'Welcome', component: WelcomeStep },
	{ id: 'download-folders', title: 'Download folders', component: DownloadFoldersStep },
	{ id: 'format', title: 'Download format', component: FormatStep },
	{ id: 'mangabaka', title: 'Metadata', component: MangaBakaStep },
	{ id: 'websites', title: 'Websites', component: WebsitesStep },
	{ id: 'finish', title: 'Finish', component: FinishStep }
];
