import { expect, test } from '@playwright/test';
import { USERDATA_ZIP } from './userdata.ts';

test('the library shows its favorites as a cover grid that chips filter', async ({ page }) => {
	await page.goto('/');
	const grid = page.getByRole('list', { name: 'Favorites' });
	const cards = grid.getByRole('link');
	await expect(cards).toHaveCount(8);
	await expect(cards.filter({ hasText: 'Frieren' })).toContainText('+4');

	const show = page.getByRole('group', { name: 'Show' });
	await show.getByRole('button', { name: /^New/ }).click();
	await expect(cards).toHaveCount(3);
	await expect(cards.filter({ hasText: 'Sakamoto Days' })).toHaveCount(0);

	await show.getByRole('button', { name: /^Completed/ }).click();
	await expect(cards).toHaveCount(1);
	await expect(cards.first()).toContainText('Vinland Saga');

	await show.getByRole('button', { name: /^All/ }).click();
	await page
		.getByRole('group', { name: 'Website' })
		.getByRole('button', { name: /^ComicK/ })
		.click();
	await expect(cards).toHaveCount(2);

	await page.getByRole('searchbox', { name: 'Search the library' }).fill('blue');
	await expect(cards).toHaveCount(1);
	await cards.first().click();
	await expect(page).toHaveURL('/series?module=comick&link=%2Fcomic%2Fblue-lock');
});

test('Check now shows the check’s progress and the new chapters it found', async ({ page }) => {
	await page.goto('/');
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	await expect(cards).toHaveCount(8);

	await page.getByRole('button', { name: 'Check now' }).click();

	await expect(page.getByRole('progressbar', { name: 'Check progress' })).toBeVisible();
	await expect(page.getByRole('status')).toContainText(/Checking \d+\/7/);
	// The mock finds one new chapter of Chainsaw Man, one favorite a second.
	await expect(page.getByRole('button', { name: 'Check now' })).toBeVisible({ timeout: 15_000 });
	await expect(cards.filter({ hasText: 'Chainsaw Man' })).toContainText('+1');
});

test('a series can be added to the library from its page', async ({ page }) => {
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fop%2Fone-piece');
	await page.getByRole('button', { name: 'Add to library' }).click();
	await expect(page.getByText('In library')).toBeVisible();

	// In-app navigation: a reload would reset the mock backend.
	await page.getByRole('link', { name: '← Library' }).click();
	await expect(
		page.getByRole('list', { name: 'Favorites' }).getByRole('link', { name: /One Piece/ })
	).toBeVisible();
});

test('a series in the library can be checked for missing chapters', async ({ page }) => {
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');
	await page.getByRole('button', { name: 'Check missing chapters' }).click();
	await expect(page.getByRole('status').filter({ hasText: 'missing chapters' })).toBeVisible();
});

test('an FMD2 userdata zip is checked with a dry run, then imported', async ({ page }) => {
	await page.goto('/');
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	await expect(cards).toHaveCount(8);

	await page.getByRole('button', { name: 'Import from FMD2' }).click();
	const dialog = page.getByRole('dialog', { name: 'Import from FMD2' });
	await dialog.getByLabel('FMD2 userdata folder, zipped').setInputFiles(USERDATA_ZIP);
	await expect(dialog.getByRole('button', { name: 'Import', exact: true })).toBeDisabled();
	await dialog.getByRole('button', { name: 'Check' }).click();

	const report = dialog.getByRole('table', { name: 'Import report' });
	await expect(dialog.getByRole('status')).toContainText('nothing was written');
	await expect(report.getByRole('row', { name: /favorites\.db/ })).toContainText('1');
	await expect(dialog.getByText(/C:\\Manga\\Guts is a Windows path/)).toBeVisible();
	await expect(dialog.getByText('connections.max_parallel_tasks: must be 1 to 8')).toBeVisible();
	await expect(cards).toHaveCount(8);

	// Mapping the Windows folder changes the options, so it needs another check.
	await dialog.getByLabel('Path maps').fill('C:\\Manga=/data/manga');
	await expect(dialog.getByRole('button', { name: 'Import', exact: true })).toBeDisabled();
	await dialog.getByRole('button', { name: 'Check' }).click();
	await expect(dialog.getByText(/is a Windows path/)).toHaveCount(0);

	await dialog.getByRole('button', { name: 'Import', exact: true }).click();
	await expect(dialog.getByRole('status')).toContainText('Imported');
	await expect(cards.filter({ hasText: 'One Piece' })).toHaveCount(1);
	await dialog.getByRole('button', { name: 'Close' }).click();
	await expect(dialog).toHaveCount(0);
});

test('an upload the server refuses shows why', async ({ page }) => {
	await page.goto('/');
	await page.getByRole('button', { name: 'Import from FMD2' }).click();
	const dialog = page.getByRole('dialog', { name: 'Import from FMD2' });
	await dialog.getByLabel('FMD2 userdata folder, zipped').setInputFiles({
		name: 'userdata.rar',
		mimeType: 'application/octet-stream',
		buffer: Buffer.from('Rar!')
	});
	await dialog.getByRole('button', { name: 'Check' }).click();
	await expect(dialog.getByRole('alert')).toContainText('not a zip file');
});
