import { expect, test } from '@playwright/test';

test('a pasted URL opens its series page, and Download queues the chosen chapters', async ({
	page
}) => {
	await page.goto('/');
	await page
		.getByRole('textbox', { name: 'Manga URL' })
		.fill('https://mangadex.org/title/abc123/frieren');
	await page.getByRole('button', { name: 'Add' }).click();

	await expect(page).toHaveURL('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Frieren');
	const chapters = page.getByRole('region', { name: 'Chapters' });
	await chapters.getByRole('checkbox', { name: 'Chapter 1', exact: true }).check();
	await chapters.getByRole('checkbox', { name: 'Chapter 2', exact: true }).check();
	const box = page.getByRole('region', { name: 'Download', exact: true });
	await expect(box).toContainText('2 chapters selected');

	await box.getByRole('button', { name: 'Download 2 chapters' }).click();

	await expect(box.getByRole('status')).toContainText('Queued: Frieren, 2 chapters');
});

test('selection shortcuts pick new chapters or a typed range', async ({ page }) => {
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');
	const box = page.getByRole('region', { name: 'Download', exact: true });
	const chapters = page.getByRole('region', { name: 'Chapters' });

	// The mock has chapters 1–138 seen.
	await chapters.getByRole('button', { name: 'New' }).click();
	await expect(box).toContainText('4 chapters selected');

	await chapters.getByRole('textbox', { name: 'Chapter range' }).fill('1-10, 20');
	await chapters.getByRole('button', { name: 'Select range' }).click();
	await expect(box).toContainText('11 chapters selected');

	await chapters.getByRole('button', { name: 'None' }).click();
	await expect(box).toContainText('No chapters selected');
});

test('a 2000-chapter list renders only what is visible and scrolls to the end', async ({
	page
}) => {
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fop%2Fone-piece');
	const list = page.getByRole('list', { name: 'Chapter list' });
	await expect(list.getByRole('checkbox', { name: 'Chapter 1', exact: true })).toBeVisible();
	expect(await list.getByRole('listitem').count()).toBeLessThan(100);

	await list.evaluate((el) => (el.scrollTop = el.scrollHeight));
	await expect(list.getByRole('checkbox', { name: 'Chapter 2000' })).toBeVisible();
});

test('an unknown series shows why it cannot be shown', async ({ page }) => {
	await page.goto('/series?module=mangadex&link=%2Fmissing');
	await expect(page.getByRole('alert')).toContainText('not found');
});

test('a slow series shows a skeleton of the page until it loads', async ({ page }) => {
	await page.addInitScript(() => sessionStorage.setItem('fmd2r.mock.series-delay-ms', '1500'));
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');

	const skeleton = page.locator('[aria-busy="true"]');
	await expect(skeleton).toBeVisible();
	await expect(page.getByText('Loading series…')).toBeAttached();
	await expect(page.getByRole('region', { name: 'Chapters' })).toHaveCount(0);
	await expect(page.getByRole('link', { name: '← Library' })).toBeVisible();

	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Frieren');
	await expect(skeleton).toHaveCount(0);
	await expect(page.getByRole('region', { name: 'Chapters' })).toBeVisible();
});

test('a check for missing chapters reports its result in the inbox', async ({ page }) => {
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');
	await page.getByRole('button', { name: 'Check missing chapters' }).click();
	await expect(page.getByRole('status').filter({ hasText: 'arrives in the inbox' })).toBeVisible();

	await page.getByRole('button', { name: /Inbox/ }).click();
	const inbox = page.getByRole('dialog', { name: 'Inbox' });
	await expect(
		inbox.getByRole('listitem').filter({ hasText: 'No missing chapters' })
	).toContainText('Checked 1 series; every chapter is on disk.');
});

test('moving the downloads along with a new folder reports in the inbox', async ({ page }) => {
	// Frieren is in the library.
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');
	const button = page.getByRole('button', { name: 'Download folder…' });
	await button.click();
	const dialog = page.getByRole('dialog', { name: 'Download folder' });
	await dialog.getByRole('textbox', { name: 'Folder' }).fill('/data/elsewhere/Frieren');
	await dialog
		.getByRole('checkbox', { name: 'Move chapters already downloaded to the new folder' })
		.check();
	await expect(dialog).toContainText('in the background');
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog).toBeHidden();
	await expect(button).toHaveAttribute('title', '/data/elsewhere/Frieren');

	await page.getByRole('button', { name: /Inbox/ }).click();
	const inbox = page.getByRole('dialog', { name: 'Inbox' });
	await expect(inbox.getByRole('listitem').first()).toContainText(
		'Moved the downloads of 1 series'
	);
});
