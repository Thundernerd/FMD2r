import { expect, test } from '@playwright/test';

test('a queued download shows up live, finishes, and its files download', async ({
	page,
	request
}) => {
	await page.goto('/queue');
	await expect(page.getByRole('heading', { level: 1, name: 'Queue' })).toBeVisible();

	const res = await request.post('/api/tasks', {
		data: {
			module_id: 'fixture',
			link: '/manga/1',
			title: 'Fixture Manga',
			chapters: [
				{ name: 'Ch. 1', link: '/c/1' },
				{ name: 'Ch. 2', link: '/c/2' }
			]
		}
	});
	expect(res.status()).toBe(201);

	// No reload: the task arrives over /api/events.
	const finished = page
		.getByRole('region', { name: /^Finished \d+$/ })
		.getByRole('listitem', { name: 'Fixture Manga' });
	await expect(finished).toBeVisible({ timeout: 30_000 });
	await expect(finished).toContainText('2/2 chapters');

	const download = page.waitForEvent('download');
	await finished.getByRole('link', { name: 'Get files' }).click();
	// Two packed chapters come as one zip named after the series.
	expect((await download).suggestedFilename()).toBe('Fixture Manga.zip');
});
