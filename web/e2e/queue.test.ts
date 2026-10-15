import { expect, test, type Page } from '@playwright/test';

const group = (page: Page, name: string) =>
	page.getByRole('region', { name: new RegExp(`^${name.replace('/', '\\/')} \\d+$`) });

test('tasks render in their status groups', async ({ page }) => {
	await page.goto('/queue');
	await expect(page.getByRole('heading', { level: 1, name: 'Queue' })).toBeVisible();
	await expect(group(page, 'Downloading').getByRole('listitem')).toHaveCount(2);
	await expect(group(page, 'Downloading')).toContainText('Kagurabachi');
	await expect(group(page, 'Waiting')).toContainText('Blue Lock');
	await expect(group(page, 'Stopped / failed')).toContainText('The Apothecary Diaries');
	await expect(group(page, 'Stopped / failed')).toContainText('HTTP 403');
	await expect(group(page, 'Finished')).toContainText('Frieren');
	await expect(page.getByRole('button', { name: 'Finished 2' })).toBeVisible();
});

test('the stop button stops a downloading task', async ({ page }) => {
	await page.goto('/queue');
	const row = group(page, 'Downloading').getByRole('listitem', { name: 'Kagurabachi' });
	await row.getByRole('button', { name: 'Stop' }).click();

	const stopped = group(page, 'Stopped / failed').getByRole('listitem', { name: 'Kagurabachi' });
	await expect(stopped).toContainText('Stopped');
	await expect(stopped.getByRole('button', { name: 'Start' })).toBeVisible();
});

test('Get files triggers a download', async ({ page }) => {
	await page.goto('/queue');
	const row = group(page, 'Finished').getByRole('listitem', { name: 'Frieren' });
	const download = page.waitForEvent('download');
	await row.getByRole('link', { name: 'Get files' }).click();
	expect(await (await download).failure()).toBeNull();
});

test('the history filter narrows by text and status', async ({ page }) => {
	await page.goto('/queue');
	await page.getByRole('searchbox', { name: 'Filter text' }).fill('frieren');
	await expect(page.getByRole('listitem')).toHaveCount(1);
	await expect(page.getByRole('listitem')).toContainText('Frieren');

	await page.getByRole('button', { name: 'Clear filter' }).click();
	await page.getByRole('button', { name: /^Finished \d+$/ }).click();
	await expect(page.getByRole('region', { name: /^Downloading/ })).toHaveCount(0);
	await expect(group(page, 'Finished').getByRole('listitem')).toHaveCount(2);
});

test('deleting a task asks whether to keep its files', async ({ page }) => {
	await page.goto('/queue');
	const row = page.getByRole('listitem', { name: 'Sakamoto Days' });
	await row.getByRole('button', { name: 'Delete…' }).click();
	await row.getByRole('button', { name: 'Delete with files' }).click();
	await expect(page.getByRole('listitem', { name: 'Sakamoto Days' })).toHaveCount(0);
});

test('the queue page fits a phone screen', async ({ page }) => {
	await page.setViewportSize({ width: 375, height: 740 });
	await page.goto('/queue');
	await expect(group(page, 'Downloading')).toContainText('Kagurabachi');
	const overflow = await page.evaluate(
		() => document.documentElement.scrollWidth - document.documentElement.clientWidth
	);
	expect(overflow).toBeLessThanOrEqual(0);
});

test('a task shows the name of the website it downloads from, not its module ID', async ({
	page
}) => {
	await page.goto('/queue');
	const row = group(page, 'Downloading').getByRole('listitem', { name: 'Kagurabachi' });
	await expect(row).toContainText('MangaDex');
	await expect(group(page, 'Finished').getByRole('listitem', { name: 'Dandadan' })).toContainText(
		'ComicK'
	);
	await expect(page.getByRole('main')).not.toContainText('mangadex');
	await expect(page.getByRole('main')).not.toContainText('comick');
});

test('Move to top puts a waiting task first', async ({ page }) => {
	await page.goto('/queue');
	const stopped = group(page, 'Stopped / failed');
	await stopped
		.getByRole('listitem', { name: 'The Apothecary Diaries' })
		.getByRole('button', { name: 'Retry' })
		.click();
	await stopped
		.getByRole('listitem', { name: 'Sakamoto Days' })
		.getByRole('button', { name: 'Start' })
		.click();
	const waiting = group(page, 'Waiting').getByRole('listitem');
	await expect(waiting).toHaveCount(3);
	const order = () => waiting.evaluateAll((rows) => rows.map((r) => r.getAttribute('aria-label')));
	expect(await order()).toEqual(['Blue Lock', 'The Apothecary Diaries', 'Sakamoto Days']);

	await waiting.nth(2).getByRole('button', { name: 'Move to top' }).click();
	await expect.poll(order).toEqual(['Sakamoto Days', 'Blue Lock', 'The Apothecary Diaries']);
});

test('the move buttons are hidden while a filter is on', async ({ page }) => {
	await page.goto('/queue');
	const row = group(page, 'Waiting').getByRole('listitem', { name: 'Blue Lock' });
	await expect(row.getByRole('button', { name: 'Move up' })).toBeVisible();

	await page.getByRole('searchbox', { name: 'Filter text' }).fill('blue');
	await expect(row.getByRole('button', { name: 'Move up' })).toHaveCount(0);
	await expect(group(page, 'Waiting')).toContainText('Clear the filter to reorder.');
});
