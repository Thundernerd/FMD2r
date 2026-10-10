import { expect, test, type Page } from '@playwright/test';
import { USERDATA_ZIP } from './userdata.ts';

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
	await expect(page.getByRole('heading', { level: 2, name: 'Import from FMD2' })).toBeVisible();
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
	await page.getByRole('link', { name: 'Change Download format in Settings' }).click();

	await expect(page).toHaveURL(/\/settings#section-output$/);
	await expect(page.getByRole('heading', { level: 2, name: 'Output' })).toBeVisible();
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
});

test('an FMD2 import during setup brings its settings to the later steps', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	const step = page.getByRole('region', { name: 'Import from FMD2' });
	await expect(step.getByText('Coming from FMD2? Import your library and settings.')).toBeVisible();

	await step.getByRole('button', { name: 'Import' }).click();
	await step.getByLabel('FMD2 userdata folder, zipped').setInputFiles(USERDATA_ZIP);
	await step.getByLabel('Path maps').fill('C:\\Manga=/data/manga');
	await expect(step.getByRole('button', { name: 'Import', exact: true })).toBeDisabled();
	await step.getByRole('button', { name: 'Check' }).click();
	await expect(step.getByRole('status')).toContainText('nothing was written');
	await expect(step.getByRole('table', { name: 'Import report' })).toBeVisible();

	await step.getByRole('button', { name: 'Import', exact: true }).click();
	await expect(step.getByRole('status')).toContainText('Imported');
	await page.getByRole('button', { name: 'Next' }).click();

	// The finish step sums up what the later steps start from: FMD2's folder, format and websites.
	await expect(page.getByText('Step 3 of 3')).toBeVisible();
	const summary = page.getByRole('region', { name: 'Finish' });
	await expect(summary.getByText('/data/manga')).toBeVisible();
	await expect(summary.getByText('CBZ', { exact: true })).toBeVisible();
	await expect(summary.getByText('2 selected')).toBeVisible();

	await page.getByRole('button', { name: 'Finish' }).click();
	await expect(
		page.getByRole('list', { name: 'Favorites' }).getByRole('link', { name: /One Piece/ })
	).toBeVisible();
});

test('a failed import keeps the user on the step, able to retry or skip', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	const step = page.getByRole('region', { name: 'Import from FMD2' });
	await step.getByRole('button', { name: 'Import' }).click();
	await step.getByLabel('FMD2 userdata folder, zipped').setInputFiles({
		name: 'userdata.rar',
		mimeType: 'application/octet-stream',
		buffer: Buffer.from('Rar!')
	});
	await step.getByRole('button', { name: 'Check' }).click();
	await expect(step.getByRole('alert')).toContainText('not a zip file');
	await expect(page.getByText('Step 2 of 3')).toBeVisible();

	await step.getByLabel('FMD2 userdata folder, zipped').setInputFiles(USERDATA_ZIP);
	await step.getByRole('button', { name: 'Check' }).click();
	await expect(step.getByRole('status')).toContainText('nothing was written');
	await page.getByRole('button', { name: 'Skip' }).click();
	await expect(page.getByText('Step 3 of 3')).toBeVisible();
});
