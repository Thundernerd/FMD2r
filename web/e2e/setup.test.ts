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
	await expect(page.getByText('Step 1 of 3')).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('Step 2 of 3')).toBeVisible();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('Step 3 of 3')).toBeVisible();
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
	await expect(page.getByText('Step 2 of 3')).toBeVisible();

	await page.reload();
	await expect(page.getByText('Step 2 of 3')).toBeVisible();
	await expect(page.getByRole('heading', { level: 2, name: 'Metadata' })).toBeVisible();
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
	await expect(page.getByText('Step 1 of 3')).toBeVisible();

	await page.goto('/discover');
	await expect(page).toHaveURL(/\/discover$/);
	await expect(wizard(page)).toHaveCount(0);
});

test("the finish step's links to Settings finish the setup first", async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('link', { name: 'Change Download format in Settings' }).click();

	await expect(page).toHaveURL(/\/settings#section-output$/);
	await expect(page.getByRole('heading', { level: 2, name: 'Output' })).toBeVisible();
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
});

test('the MangaBaka database downloads in the background while setup goes on', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('390 MB')).toBeVisible();
	await page.getByRole('button', { name: 'Download now' }).click();
	await expect(page.getByRole('progressbar', { name: 'Download progress' })).toBeVisible();

	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByRole('heading', { level: 2, name: 'Finish' })).toBeVisible();
	await expect(page.getByText('Still downloading')).toBeVisible();
	await page.getByRole('link', { name: 'Change MangaBaka database in Settings' }).click();

	await expect(page).toHaveURL(/\/settings#section-metadata$/);
	await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();
	await expect(page.getByRole('progressbar', { name: 'Download progress' })).toBeVisible();
});
