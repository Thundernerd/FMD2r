import { expect, test, type Page } from '@playwright/test';
import { USERDATA_ZIP } from './userdata.ts';

/** Makes the mock backend a fresh install, which has not been through setup yet. */
const freshInstall = (page: Page) =>
	page.addInitScript(() => sessionStorage.setItem('fmd2r.mock.fresh-install', '1'));

const wizard = (page: Page) => page.getByRole('heading', { level: 1, name: 'Set up FMD2r' });

/**
 * Goes from the welcome to the websites step, skipping the import and MangaBaka and keeping the
 * defaults.
 */
async function toWebsites(page: Page) {
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByRole('heading', { level: 2, name: 'Websites' })).toBeVisible();
}

/** Chooses MangaDex on the websites step and goes on to the finish. */
async function pastWebsites(page: Page) {
	await page.getByRole('checkbox', { name: 'MangaDex' }).check();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByRole('heading', { level: 2, name: 'Finish' })).toBeVisible();
}

/** Goes from the welcome to the finish. */
async function toFinish(page: Page) {
	await toWebsites(page);
	await pastWebsites(page);
}

test('a fresh install goes through setup first, and only once', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/discover');
	await expect(wizard(page)).toBeVisible();
	await expect(page).toHaveURL(/\/setup$/);
	// Nothing else of the app while it shows.
	await expect(page.getByRole('navigation', { name: 'Main' })).toHaveCount(0);

	await page.goto('/');
	await expect(wizard(page)).toBeVisible();
	await expect(page.getByText('Step 1 of 7')).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('Step 2 of 7')).toBeVisible();
	await page.getByRole('button', { name: 'Skip' }).click();
	await expect(page.getByText('Step 3 of 7')).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('Step 4 of 7')).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('Step 5 of 7')).toBeVisible();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('Step 6 of 7')).toBeVisible();
	await pastWebsites(page);
	await expect(page.getByText('Step 7 of 7')).toBeVisible();
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
	await expect(page.getByText('Step 2 of 7')).toBeVisible();

	await page.reload();
	await expect(page.getByText('Step 2 of 7')).toBeVisible();
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
	await expect(page.getByText('Step 1 of 7')).toBeVisible();

	await page.goto('/discover');
	await expect(page).toHaveURL(/\/discover$/);
	await expect(wizard(page)).toHaveCount(0);
});

test("the finish step's links to Settings finish the setup first", async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await toFinish(page);
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
	await page.getByRole('button', { name: 'Skip' }).click();
	const step = page.getByRole('region', { name: 'Download folders' });
	await expect(step.getByRole('group', { name: 'Downloads' })).toBeVisible();
	await step.getByRole('button', { name: 'Add destination' }).click();
	const added = step.getByRole('group', { name: 'New destination' });
	await added.getByRole('textbox', { name: 'Folder' }).fill('/data/manhwa');
	await added.getByRole('textbox', { name: 'Name' }).fill('Manhwa');
	await step.getByRole('group', { name: 'Manhwa' }).getByRole('radio', { name: 'Default' }).check();
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await pastWebsites(page);
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

test('the download format picked during setup shows in Settings → Output', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByRole('heading', { level: 2, name: 'Download format' })).toBeVisible();
	await expect(page.getByRole('radio', { name: /^Folder of images/ })).toBeChecked();
	await page.getByRole('radio', { name: /^CBZ/ }).check();
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await pastWebsites(page);
	await page.getByRole('button', { name: 'Finish' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();

	await page.goto('/settings#section-output');
	await expect(page.getByLabel('Save chapters as').locator('option:checked')).toHaveText('CBZ');
});

test('the MangaBaka database downloads in the background while setup goes on', async ({ page }) => {
	await freshInstall(page);
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByText('390 MB')).toBeVisible();
	await page.getByRole('button', { name: 'Download now' }).click();
	await expect(page.getByRole('progressbar', { name: 'Download progress' })).toBeVisible();

	await page.getByRole('button', { name: 'Next' }).click();
	await pastWebsites(page);
	await expect(page.getByRole('heading', { level: 2, name: 'Finish' })).toBeVisible();
	await expect(page.getByText('Still downloading')).toBeVisible();
	await page.getByRole('link', { name: 'Change MangaBaka database in Settings' }).click();

	await expect(page).toHaveURL(/\/settings#section-metadata$/);
	await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();
	await expect(page.getByRole('progressbar', { name: 'Download progress' })).toBeVisible();
});

test('the websites chosen during setup are the ones Discover lists', async ({ page }, info) => {
	test.skip(info.project.name === 'phone', 'the filters are a drawer on a phone');
	await freshInstall(page);
	await page.goto('/');
	await toWebsites(page);
	await expect(page.getByRole('button', { name: 'Next' })).toBeDisabled();
	await page.getByRole('checkbox', { name: 'MangaDex' }).check();
	await page.getByRole('checkbox', { name: 'Webtoons' }).check();
	await page.getByRole('button', { name: 'Next' }).click();

	// The finish step points to Discover, where each website's list is fetched; going there
	// finishes the setup.
	await expect(page.getByRole('heading', { level: 2, name: 'Finish' })).toBeVisible();
	await page.getByRole('link', { name: 'Discover' }).click();
	await expect(page).toHaveURL(/\/discover$/);
	await expect(wizard(page)).toHaveCount(0);

	const website = page
		.getByRole('complementary', { name: 'Filters' })
		.getByRole('combobox', { name: 'Website' });
	await expect(website.getByRole('option')).toHaveText(['All websites', 'MangaDex', 'Webtoons']);
});

/** Makes the mock backend listen where other machines can reach it, with no password. */
const openServer = (page: Page) =>
	page.addInitScript(() => sessionStorage.setItem('fmd2r.mock.open', '1'));

const banner = (page: Page) => page.getByRole('alert').filter({ hasText: 'no password' });

/** Goes from the welcome to the password step, which follows the websites on an open server. */
async function toPassword(page: Page) {
	await toWebsites(page);
	await page.getByRole('checkbox', { name: 'MangaDex' }).check();
	await page.getByRole('button', { name: 'Next' }).click();
	await expect(page.getByRole('heading', { level: 2, name: 'Password' })).toBeVisible();
}

test('an open server can set a password during setup without logging out', async ({ page }) => {
	await freshInstall(page);
	await openServer(page);
	await page.goto('/');
	await expect(page.getByText('Step 1 of 8')).toBeVisible();
	await toPassword(page);
	await expect(page.getByText('Step 7 of 8')).toBeVisible();
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
	await toPassword(page);
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Finish' }).click();

	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
	await expect(banner(page)).toBeVisible();
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

	// The later steps start from FMD2's download folder, format and websites, and say so.
	const fromFmd2 = page.getByText('These start from your FMD2 settings');
	const folders = page.getByRole('region', { name: 'Download folders' });
	await expect(
		folders.getByRole('group', { name: 'Downloads' }).getByRole('textbox', { name: 'Folder' })
	).toHaveValue('/data/manga');
	await expect(fromFmd2).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();

	await expect(page.getByRole('heading', { level: 2, name: 'Download format' })).toBeVisible();
	await expect(page.getByRole('radio', { name: /^CBZ/ })).toBeChecked();
	await expect(fromFmd2).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();

	await expect(page.getByRole('heading', { level: 2, name: 'Metadata' })).toBeVisible();
	await expect(fromFmd2).toHaveCount(0);
	await page.getByRole('button', { name: 'Skip' }).click();
	await page.getByRole('button', { name: 'Next' }).click();

	await expect(page.getByRole('heading', { level: 2, name: 'Websites' })).toBeVisible();
	await expect(page.getByRole('checkbox', { name: 'MangaDex' })).toBeChecked();
	await expect(page.getByRole('checkbox', { name: 'ComicK' })).toBeChecked();
	await expect(page.getByRole('checkbox', { name: 'Webtoons' })).not.toBeChecked();
	await expect(fromFmd2).toBeVisible();
	await page.getByRole('button', { name: 'Next' }).click();

	const summary = page.getByRole('region', { name: 'Finish' });
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
	await expect(page.getByText('Step 2 of 7')).toBeVisible();

	await step.getByLabel('FMD2 userdata folder, zipped').setInputFiles(USERDATA_ZIP);
	await step.getByRole('button', { name: 'Check' }).click();
	await expect(step.getByRole('status')).toContainText('nothing was written');
	await page.getByRole('button', { name: 'Skip' }).click();
	await expect(page.getByRole('heading', { level: 2, name: 'Download folders' })).toBeVisible();
});
