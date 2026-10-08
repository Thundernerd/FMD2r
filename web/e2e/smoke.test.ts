import { expect, test } from '@playwright/test';

test('app loads with the Library page and the queue dock', async ({ page }) => {
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
	await expect(page.getByRole('region', { name: 'Download queue' })).toContainText('Kagurabachi');
});

test('main navigation reaches every section', async ({ page }) => {
	await page.goto('/');
	const nav = page.getByRole('navigation', { name: 'Main' });
	for (const [link, path, heading] of [
		['Discover', '/discover', 'Discover'],
		['Queue', '/queue', 'Queue'],
		['Settings', '/settings', 'Settings'],
		['System', '/system', 'System'],
		['Library', '/', 'Library']
	] as const) {
		await nav.getByRole('link', { name: link }).click();
		await expect(page).toHaveURL(path);
		await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
		await expect(nav.getByRole('link', { name: link })).toHaveAttribute('aria-current', 'page');
	}
});

test('inbox popover opens and marks items read', async ({ page }) => {
	await page.goto('/');
	const bell = page.getByRole('button', { name: /Inbox/ });
	await expect(bell).toContainText('3');

	await bell.click();
	const popover = page.getByRole('dialog', { name: 'Inbox' });
	await expect(popover).toBeVisible();
	const item = popover.getByRole('listitem').filter({ hasText: 'New chapters for 3 favorites' });
	await item.getByRole('button', { name: 'Mark read' }).click();

	await expect(bell).toContainText('2');
	await expect(item.getByRole('button', { name: 'Mark read' })).toHaveCount(0);
});

test('add by URL opens the series page', async ({ page }) => {
	await page.goto('/');
	await page
		.getByRole('textbox', { name: 'Manga URL' })
		.fill('https://mangadex.org/title/abc123/frieren');
	await page.getByRole('button', { name: 'Add' }).click();

	await expect(page).toHaveURL('/series/MangaDex/title/abc123/frieren');
	await expect(page.getByRole('heading', { level: 1 })).toContainText('title/abc123/frieren');
});

test('add by URL reports a URL no module handles', async ({ page }) => {
	await page.goto('/');
	await page.getByRole('textbox', { name: 'Manga URL' }).fill('https://example.com/nothing');
	await page.getByRole('button', { name: 'Add' }).click();

	await expect(page.getByRole('alert')).toContainText('No module');
	await expect(page).toHaveURL('/');
});
