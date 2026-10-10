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
	await expect(page.getByRole('heading', { level: 2, name: 'Download folders' })).toBeVisible();
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
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('link', { name: 'Change Download format in Settings' }).click();

	await expect(page).toHaveURL(/\/settings#section-output$/);
	await expect(page.getByRole('heading', { level: 2, name: 'Output' })).toBeVisible();
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
});

test('download folders added during setup show in Settings → Save to', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	const step = page.getByRole('region', { name: 'Download folders' });
	await expect(step.getByRole('group', { name: 'Downloads' })).toBeVisible();
	await step.getByRole('button', { name: 'Add destination' }).click();
	const added = step.getByRole('group', { name: 'New destination' });
	await added.getByRole('textbox', { name: 'Folder' }).fill('/data/manhwa');
	await added.getByRole('textbox', { name: 'Name' }).fill('Manhwa');
	await step.getByRole('group', { name: 'Manhwa' }).getByRole('radio', { name: 'Default' }).check();
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Finish' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();

	await page.goto('/settings#section-saveto');
	const section = page.getByRole('region', { name: 'Save to' });
	const downloads = section.getByRole('group', { name: 'Downloads' });
	await expect(downloads.getByRole('textbox', { name: 'Folder' })).toHaveValue('downloads');
	await expect(downloads.getByRole('radio', { name: 'Default' })).not.toBeChecked();
	const manhwa = section.getByRole('group', { name: 'Manhwa' });
	await expect(manhwa.getByRole('textbox', { name: 'Folder' })).toHaveValue('/data/manhwa');
	await expect(manhwa.getByRole('radio', { name: 'Default' })).toBeChecked();
});
