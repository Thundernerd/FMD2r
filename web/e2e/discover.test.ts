import { expect, test } from '@playwright/test';

test('a website’s list can be searched and filtered with tri-state genres', async ({
	page
}, info) => {
	test.skip(info.project.name === 'phone', 'the filters are a drawer on a phone');
	await page.goto('/discover');
	const results = page.getByRole('region', { name: 'Results' });
	const filters = page.getByRole('complementary', { name: 'Filters' });
	const count = results.getByRole('status');

	await filters.getByRole('combobox', { name: 'Website' }).selectOption({ label: 'MangaDex' });
	await expect(count).toHaveText('140 titles');
	await expect(filters.getByRole('group', { name: 'List of MangaDex' })).toContainText(
		'140 titles'
	);

	// The chip counts the titles carrying the genre.
	const action = filters.getByRole('button', { name: 'Action: ignored' });
	const included = Number(await action.locator('.count').textContent());
	expect(included).toBeGreaterThan(0);
	expect(included).toBeLessThan(140);

	await action.click();
	await expect(filters.getByRole('button', { name: 'Action: included' })).toBeVisible();
	await expect(count).toHaveText(`${included} titles`);

	await filters.getByRole('button', { name: 'Action: included' }).click();
	await expect(filters.getByRole('button', { name: 'Action: excluded' })).toBeVisible();
	await expect(count).toHaveText(`${140 - included} titles`);

	await filters.getByRole('button', { name: 'Action: excluded' }).click();
	await expect(filters.getByRole('button', { name: 'Action: ignored' })).toBeVisible();
	await expect(count).toHaveText('140 titles');

	await results.getByRole('searchbox', { name: 'Search titles' }).fill('zzz');
	await expect(count).toHaveText('0 titles');
	await expect(results).toContainText('No titles match');
});

test('more results load as the list scrolls, and a title opens its series page', async ({
	page
}) => {
	await page.goto('/discover');
	const results = page.getByRole('region', { name: 'Results' });
	const cards = results.getByRole('link');
	await expect(cards.first()).toBeVisible();
	await expect(cards).toHaveCount(50);

	await results.getByRole('button', { name: 'Load more' }).scrollIntoViewIfNeeded();
	await expect(cards).toHaveCount(100);

	const first = cards.first();
	const href = await first.getAttribute('href');
	expect(href).toMatch(/^\/series\?module=[^&]+&link=%2Fmanga%2F[^&]+$/);
	await first.click();
	await expect(page).toHaveURL(href ?? '');
});

test('a website without a list gets one from FMD2-DB with live progress', async ({
	page
}, info) => {
	test.skip(info.project.name === 'phone', 'the filters are a drawer on a phone');
	await page.goto('/discover');
	const filters = page.getByRole('complementary', { name: 'Filters' });
	await filters.getByRole('combobox', { name: 'Website' }).selectOption({ label: 'Bato.to' });
	const list = filters.getByRole('group', { name: 'List of Bato.to' });
	await expect(list).toContainText('No list yet');
	await expect(page.getByRole('region', { name: 'Results' })).toContainText(
		'Bato.to has no list yet'
	);

	await list.getByRole('button', { name: 'Get from FMD2-DB' }).click();
	await expect(list.getByRole('progressbar', { name: 'List job progress' })).toBeVisible();
	await expect(list).toContainText('Imported 60 titles.');
	await expect(list).toContainText('60 titles');
	await expect(page.getByRole('region', { name: 'Results' }).getByRole('status')).toHaveText(
		'60 titles'
	);
});

test('on a phone the filters open in a drawer', async ({ page }, info) => {
	test.skip(info.project.name !== 'phone', 'the filters are a sidebar on a desktop');
	await page.goto('/discover');
	const filters = page.getByRole('complementary', { name: 'Filters' });
	await expect(filters).toBeHidden();

	await page.getByRole('button', { name: 'Filters' }).click();
	await expect(filters).toBeVisible();
	await filters.getByRole('combobox', { name: 'Website' }).selectOption({ label: 'ComicK' });
	await filters.getByRole('button', { name: 'Done' }).click();
	await expect(filters).toBeHidden();
	await expect(page.getByRole('region', { name: 'Results' }).getByRole('status')).toHaveText(
		'90 titles'
	);
});

test('Discover lists only the websites selected in Settings', async ({ page }, info) => {
	await page.goto('/settings#section-websites');
	const websites = page.getByRole('region', { name: 'Websites', exact: true });
	await websites.getByRole('button', { name: 'Select none' }).click();
	await expect(websites.getByRole('status')).toHaveText(/^0 of \d+ websites selected$/);
	await websites.getByRole('checkbox', { name: 'MangaDex' }).check();
	await websites.getByRole('checkbox', { name: 'Webtoons' }).check();
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');

	await page.goto('/discover');
	// On a phone the picker is in the filters drawer.
	if (info.project.name === 'phone') await page.getByRole('button', { name: 'Filters' }).click();
	const picker = page.getByRole('combobox', { name: 'Website' });
	await expect(picker.getByRole('option')).toHaveText(['All websites', 'MangaDex', 'Webtoons']);
	// Searching every selected website covers only their lists (140 and 48 titles).
	await expect(page.getByRole('region', { name: 'Results' }).getByRole('status')).toHaveText(
		'188 titles'
	);
});

test('with no website selected Discover links to the selection', async ({ page }) => {
	await page.goto('/settings#section-websites');
	const websites = page.getByRole('region', { name: 'Websites', exact: true });
	await websites.getByRole('button', { name: 'Select none' }).click();
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');

	await page.goto('/discover');
	await expect(page.getByText('No websites are selected.')).toBeVisible();
	await page.getByRole('link', { name: 'Choose websites' }).click();
	await expect(page).toHaveURL(/\/settings#section-websites$/);
	await expect(websites).toBeInViewport();
});

test('the MangaBaka database is downloaded on request and adds format facets', async ({
	page
}, info) => {
	test.skip(info.project.name === 'phone', 'the filters are a drawer on a phone');
	await page.goto('/discover');
	const filters = page.getByRole('complementary', { name: 'Filters' });
	await expect(filters.getByRole('combobox', { name: 'Status' })).toBeVisible();
	await expect(filters.getByRole('combobox', { name: 'Format' })).toHaveCount(0);

	await page.getByRole('note').getByRole('link', { name: 'Set up the MangaBaka database' }).click();
	await expect(page.getByRole('heading', { name: 'MangaBaka database' })).toBeVisible();
	await expect(page.getByText('Not downloaded.')).toBeVisible();
	await page.getByRole('button', { name: 'Download' }).click();
	await expect(page.getByRole('progressbar')).toBeVisible();
	await expect(page.getByText(/^Built /)).toBeVisible({ timeout: 15_000 });
	await expect(page.getByRole('button', { name: 'Remove' })).toBeVisible();

	await page.getByRole('link', { name: 'Discover' }).first().click();
	await expect(filters.getByRole('combobox', { name: 'Format' })).toBeVisible();
	await expect(filters.getByRole('combobox', { name: 'Publication' })).toBeVisible();
	await expect(page.getByRole('note')).toHaveCount(0);
});

/** A 1x1 PNG. */
const PIXEL = Buffer.from(
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
	'base64'
);

test('scrolling Discover loads the covers of the visible cards only', async ({ page }) => {
	const requested: string[] = [];
	await page.route('**/api/covers/series?**', (route) => {
		requested.push(new URL(route.request().url()).searchParams.get('link') ?? '');
		return route.fulfill({ contentType: 'image/png', body: PIXEL });
	});
	await page.goto('/discover');
	const cards = page.getByRole('region', { name: 'Results' }).getByRole('link');
	await expect(cards).toHaveCount(50);
	await expect(cards.first().locator('img.loaded')).toBeVisible();

	const onScreen = requested.length;
	expect(onScreen).toBeGreaterThan(0);
	expect(onScreen).toBeLessThan(50);
	const last = cards.nth(49);
	const lastLink = new URL((await last.getAttribute('href')) ?? '', page.url()).searchParams.get(
		'link'
	);
	expect(requested).not.toContain(lastLink);

	await last.scrollIntoViewIfNeeded();
	await expect(last.locator('img.loaded')).toBeVisible();
	expect(requested).toContain(lastLink);
});
