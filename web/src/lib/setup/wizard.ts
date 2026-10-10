import FinishStep from '#lib/components/setup/FinishStep.svelte';
import PasswordStep from '#lib/components/setup/PasswordStep.svelte';
import WelcomeStep from '#lib/components/setup/WelcomeStep.svelte';
import type { Health } from '#lib/api/types.ts';
import type { SetupStep } from './steps.ts';

/** Anyone who can reach the server can use it, and the UI can set its password. */
export const isOpen = (health: Health): boolean =>
	!health.auth && !health.loopback && !health.overridden.includes('server.auth_token');

/**
 * The setup wizard's steps, in order. Each step ticket adds its entry here: after the welcome,
 * import from FMD2 (T84), download folders (T79), download format (T80), MangaBaka (T81),
 * websites (T82) and password (T83, only when the server is open), then the finish.
 */
export const SETUP_STEPS: SetupStep[] = [
	{ id: 'welcome', title: 'Welcome', component: WelcomeStep },
	// Last before the finish: setting a password ends every session, this one included.
	{ id: 'password', title: 'Password', component: PasswordStep, shows: isOpen },
	{ id: 'finish', title: 'Finish', component: FinishStep }
];
