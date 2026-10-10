import { expect, test, type Page } from '@playwright/test';

/** Makes the mock backend a fresh install, which has not been through setup yet. */
const freshInstall = (page: Page) =>
	page.addInitScript(() => sessionStorage.setItem('fmd2r.mock.fresh-install', '1'));

const wizard = (page: Page) => page.getByRole('heading', { level: 1, name: 'Set up FMD2r' });

test('a fresh install goes through setup first, and only once', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/discover');
	await expect(wizard(page)).toBeVisible();
	await expect(page).toHaveURL(/\/setup$/);
	// Nothing else of the app while it shows.
	await expect(page.getByRole('navigation', { name: 'Main' })).toHaveCount(0);

	await page.goto('/');
	await expect(wizard(page)).toBeVisible();
	await expect(page.getByText('Step 1 of 2')).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('Step 2 of 2')).toBeVisible();
	await page.getByRole('button', { name: 'Finish' }).click();

	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
	await expect(page).toHaveURL(/\/$/);
	await page.goto('/discover');
	await expect(page).toHaveURL(/\/discover$/);
	await page.reload();
	await expect(page).toHaveURL(/\/discover$/);
	await expect(wizard(page)).toHaveCount(0);
});

test('a reload during setup resumes at the step it was on', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('Step 2 of 2')).toBeVisible();

	await page.reload();
	await expect(page.getByText('Step 2 of 2')).toBeVisible();
	await expect(page.getByRole('heading', { level: 2, name: 'Finish' })).toBeVisible();
});

test('an existing install never sees setup', async ({ page }) => {
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
	await page.goto('/discover');
	await expect(page).toHaveURL(/\/discover$/);
	await expect(wizard(page)).toHaveCount(0);
});

test('setup can be run again from Settings without redirecting other pages', async ({ page }) => {
	await page.goto('/settings#section-general');
	await page.getByRole('button', { name: 'Run setup again' }).click();
	await expect(wizard(page)).toBeVisible();
	await expect(page.getByText('Step 1 of 2')).toBeVisible();

	await page.goto('/discover');
	await expect(page).toHaveURL(/\/discover$/);
	await expect(wizard(page)).toHaveCount(0);
});

test("the finish step's links to Settings finish the setup first", async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('link', { name: 'Change Download format in Settings' }).click();

	await expect(page).toHaveURL(/\/settings#section-output$/);
	await expect(page.getByRole('heading', { level: 2, name: 'Output' })).toBeVisible();
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
});

/** Makes the mock backend listen where other machines can reach it, with no password. */
const openServer = (page: Page) =>
	page.addInitScript(() => sessionStorage.setItem('fmd2r.mock.open', '1'));

const banner = (page: Page) => page.getByRole('alert').filter({ hasText: 'no password' });

test('an open server can set a password during setup without logging out', async ({ page }) => {
	await freshInstall(page);
	await openServer(page);
	await page.goto('/');
	await expect(page.getByText('Step 1 of 3')).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByRole('heading', { level: 2, name: 'Password' })).toBeVisible();
	await page.getByRole('textbox', { name: 'Password', exact: true }).fill('hunter2');
	await page.getByLabel('Confirm password').fill('hunter2');
	await page.getByRole('button', { name: 'Next' }).click();

	await expect(page.getByRole('heading', { level: 2, name: 'Finish' })).toBeVisible();
	await page.getByRole('button', { name: 'Finish' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
	await expect(page.getByRole('heading', { name: 'Log in' })).toHaveCount(0);
	await expect(banner(page)).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Log out' })).toBeVisible();
});

test('skipping the password step leaves the open server warning', async ({ page }) => {
	await freshInstall(page);
	await openServer(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Finish' }).click();

	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
	await expect(banner(page)).toBeVisible();
});
