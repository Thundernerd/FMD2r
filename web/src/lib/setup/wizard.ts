import FinishStep from '#lib/components/setup/FinishStep.svelte';
import ImportStep from '#lib/components/setup/ImportStep.svelte';
import WelcomeStep from '#lib/components/setup/WelcomeStep.svelte';
import type { SetupStep } from './steps.ts';

/**
 * The setup wizard's steps, in order. Each step ticket adds its entry here: after the welcome,
 * import from FMD2 (T84), download folders (T79), download format (T80), MangaBaka (T81),
 * websites (T82) and password (T83, only when the server is open), then the finish. A step that
 * edits settings an FMD2 import brings (`saveto.default_dir`, which moves the default
 * destination, `saveto.destinations`, `output.format`, `general.selected_websites`) lists them in `paths`, so it says when it starts from them.
 */
export const SETUP_STEPS: SetupStep[] = [
	{ id: 'welcome', title: 'Welcome', component: WelcomeStep },
	{ id: 'import', title: 'Import from FMD2', component: ImportStep },
	{ id: 'finish', title: 'Finish', component: FinishStep }
];
