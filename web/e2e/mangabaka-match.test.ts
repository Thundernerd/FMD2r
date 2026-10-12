import { expect, test } from '@playwright/test';

test('an unmatched series is matched to a MangaBaka series found by title', async ({ page }) => {
	await page.addInitScript(() => sessionStorage.setItem('fmd2r.mock.mangabaka', '1'));
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fsd%2Fsakamoto-days');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Sakamoto Days');
	const match = page.getByRole('group', { name: 'MangaBaka match' });
	await expect(match).toContainText('Not matched on MangaBaka');

	await match.getByRole('button', { name: 'Match…' }).click();
	const dialog = page.getByRole('dialog', { name: 'Match on MangaBaka' });
	const search = dialog.getByRole('searchbox', { name: 'Search MangaBaka' });
	await expect(search).toHaveValue('Sakamoto Days');
	await search.fill('sakamoto');
	const results = dialog.getByRole('list', { name: 'MangaBaka series' });
	await expect(results.getByRole('button')).toHaveCount(2);
	await results.getByRole('button', { name: /Sakamoto Days/ }).click();

	await expect(dialog).toBeHidden();
	const facts = page.locator('.facts');
	await expect(facts).toContainText('Year');
	await expect(facts).toContainText('2020');
	await expect(page.getByRole('link', { name: 'MangaBaka', exact: true })).toHaveAttribute(
		'href',
		'https://mangabaka.org/9314'
	);
	await expect(match).toContainText('matched by hand');

	await match.getByRole('button', { name: 'Undo' }).click();
	await expect(match).toContainText('Not matched on MangaBaka');
});

test('without a MangaBaka database the series page offers no matching', async ({ page }) => {
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fsd%2Fsakamoto-days');
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Sakamoto Days');
	await expect(page.getByRole('group', { name: 'MangaBaka match' })).toHaveCount(0);
});
