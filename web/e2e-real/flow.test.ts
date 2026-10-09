import { readFile } from 'node:fs/promises';
import { basename } from 'node:path';

import { expect, test } from '@playwright/test';

import { SITE } from './site.ts';
import { unzip } from './zip.ts';

/** The width of PNG `png`; the fixture site's page `n` is `n` pixels wide. */
const pngWidth = (png: Buffer) => png.readUInt32BE(16);

// The plan's end-to-end run (docs/plan.md, Verification): a series from a pasted URL to its
// files on disk, through a server restart. Each project uses its own series, so the runs share
// the one server without seeing each other's library or downloads.
test('add by URL, download two chapters through a restart, then check for new chapters', async ({
	page,
	request
}, testInfo) => {
	const key = testInfo.project.name;
	const title = `Fixture ${key}`;

	await page.goto('/');
	await page.getByRole('textbox', { name: 'Manga URL' }).fill(`${SITE}/manga/${key}`);
	await page.getByRole('button', { name: 'Add', exact: true }).click();

	await expect(page.getByRole('heading', { level: 1, name: title })).toBeVisible();
	// The cover comes through the server's cover proxy and loads.
	const cover = page.locator('img.cover');
	await expect(cover).toHaveAttribute('src', /^\/api\/covers\?/);
	await expect.poll(() => cover.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(4);
	// Everything from here on happens in this one page load: a reload would drop the mark.
	await page.evaluate(() => (document.documentElement.dataset.e2e = 'loaded once'));

	await page.getByRole('button', { name: 'Add to library' }).click();
	await expect(page.getByText('★ In library')).toBeVisible();

	await page.getByRole('checkbox', { name: 'Ch. 1' }).check();
	await page.getByRole('checkbox', { name: 'Ch. 2' }).check();
	await page.getByRole('button', { name: 'Download 2 chapters' }).click();
	await page.getByRole('link', { name: 'Open queue' }).click();

	// Chapter 1 finishes while chapter 2's pages are held: progress arrives over /api/events.
	const downloading = page
		.getByRole('region', { name: /^Downloading \d+$/ })
		.getByRole('listitem', { name: title });
	await expect(downloading).toContainText('1/2 chapters', { timeout: 30_000 });
	await expect(page.locator('html')).toHaveAttribute('data-e2e', 'loaded once');

	// Restarted mid-download, the server resumes the task; the page reconnects on its own.
	expect((await request.post(`${SITE}/restart`, { timeout: 60_000 })).ok()).toBe(true);
	expect((await request.post(`${SITE}/release/${key}`)).ok()).toBe(true);
	const finished = page
		.getByRole('region', { name: /^Finished \d+$/ })
		.getByRole('listitem', { name: title });
	await expect(finished).toContainText('2/2 chapters', { timeout: 60_000 });

	const download = page.waitForEvent('download');
	await finished.getByRole('link', { name: 'Get files' }).click();
	// Two packed chapters come as one zip named after the series, holding a CBZ per chapter,
	// named with FMD2's default 3-digit chapter numbers.
	const files = await download;
	expect(files.suggestedFilename()).toBe(`${title}.zip`);
	const archives = unzip(await readFile(await files.path()));
	expect(archives.map((a) => basename(a.name))).toEqual(['Ch. 001.cbz', 'Ch. 002.cbz']);
	for (const archive of archives) {
		const pages = unzip(archive.data);
		// Every page, in natural order: 2 before 10.
		expect(pages.map((p) => pngWidth(p.data))).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
	}

	// The site lists a new chapter; the library check finds it and says so in the inbox.
	expect((await request.post(`${SITE}/publish/${key}`)).ok()).toBe(true);
	await page
		.getByRole('navigation', { name: 'Main' })
		.getByRole('link', { name: 'Library' })
		.click();
	await page.getByRole('button', { name: 'Check now' }).click();
	await page.getByRole('button', { name: 'Inbox' }).click();
	await expect(
		page
			.getByRole('dialog', { name: 'Inbox' })
			.getByRole('listitem')
			.filter({ hasText: `${title} <Fixture> has 1 new chapter(s).` })
	).toContainText('Found new chapter(s)', { timeout: 30_000 });
	await expect(page.locator('html')).toHaveAttribute('data-e2e', 'loaded once');
});
