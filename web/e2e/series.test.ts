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

	// The mock answers with the task it queued, named after the chapters it was sent.
	await expect(box.getByRole('status')).toContainText('Chapter 1, Chapter 2');
});

test('selection shortcuts pick new chapters or a typed range', async ({ page }) => {
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');
	const box = page.getByRole('region', { name: 'Download', exact: true });
	const chapters = page.getByRole('region', { name: 'Chapters' });

	// The mock has chapters 1–138 downloaded.
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
