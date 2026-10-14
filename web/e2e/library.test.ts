import { expect, test, type Locator, type Page } from '@playwright/test';
import { USERDATA_ZIP } from './userdata.ts';

test('the library shows its favorites as a cover grid that chips filter', async ({ page }) => {
	await page.goto('/');
	const grid = page.getByRole('list', { name: 'Favorites' });
	const cards = grid.getByRole('link');
	await expect(cards).toHaveCount(8);
	await expect(cards.filter({ hasText: 'Frieren' })).toContainText('+4');
	await expect(page.getByRole('button', { name: /^Download folder of / })).toHaveCount(0);

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

test('the library filters by tag and format, with counts', async ({ page }) => {
	await page.goto('/');
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	await expect(cards).toHaveCount(8);

	const tags = page.getByRole('group', { name: 'Tags' });
	const action = tags.getByRole('button', { name: 'Action: ignored' });
	await expect(action.locator('.count')).toHaveText('2');
	await action.click();
	await expect(cards).toHaveCount(2);
	await expect(cards.filter({ hasText: 'Kagurabachi' })).toHaveCount(1);

	const format = page.getByRole('combobox', { name: 'Format' });
	await expect(format.getByRole('option', { name: 'Manhwa (1)' })).toHaveCount(1);
	await expect(format.getByRole('option', { name: 'Unknown (6)' })).toHaveCount(1);
	await format.selectOption({ label: 'Manhwa (1)' });
	await expect(cards).toHaveCount(1);
	await expect(cards.first()).toContainText('Omniscient Reader');

	// A second click excludes the tag.
	await format.selectOption({ label: 'Any' });
	await tags.getByRole('button', { name: 'Action: included' }).click();
	await expect(cards).toHaveCount(6);
	await expect(cards.filter({ hasText: 'Kagurabachi' })).toHaveCount(0);
});

test('a library card shows its chapter count, clear of its other badges', async ({ page }) => {
	await page.goto('/');
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	const frieren = cards.filter({ hasText: 'Frieren' });
	const count = frieren.getByLabel('142 chapters');
	await expect(count).toHaveText('142');
	await expectApart(count, frieren.getByLabel('4 new chapters'));
	const apothecary = cards.filter({ hasText: 'The Apothecary Diaries' });
	await expectApart(apothecary.getByLabel('76 chapters'), apothecary.getByText('Not found'));

	await page.getByRole('button', { name: 'Select' }).click();
	const selectable = page
		.getByRole('list', { name: 'Favorites' })
		.getByRole('checkbox', { name: 'Frieren' });
	await expectApart(selectable.getByLabel('142 chapters'), selectable.locator('.tick'));
});

/** Expects `a` and `b` both on screen, without overlapping. */
async function expectApart(a: Locator, b: Locator) {
	await expect(a).toBeVisible();
	await expect(b).toBeVisible();
	const [boxA, boxB] = await Promise.all([a.boundingBox(), b.boundingBox()]);
	if (!boxA || !boxB) throw new Error('a badge has no box');
	const apart =
		boxA.x + boxA.width <= boxB.x ||
		boxB.x + boxB.width <= boxA.x ||
		boxA.y + boxA.height <= boxB.y ||
		boxB.y + boxB.height <= boxA.y;
	expect(apart).toBe(true);
}

/** A 1x1 PNG. */
const PIXEL = Buffer.from(
	'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=',
	'base64'
);

test('library covers load as thumbnails, and a missing one shows the title', async ({ page }) => {
	const requested: URL[] = [];
	await page.route(
		(url) => url.pathname.startsWith('/api/covers'),
		(route) => {
			const url = new URL(route.request().url());
			requested.push(url);
			// Sakamoto Days has no cover anywhere.
			return url.searchParams.get('link') === '/title/sd/sakamoto-days'
				? route.fulfill({ status: 404 })
				: route.fulfill({ contentType: 'image/png', body: PIXEL });
		}
	);
	await page.goto('/');
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	await expect(cards).toHaveCount(8);

	await expect(cards.filter({ hasText: 'Frieren' }).locator('img.loaded')).toBeVisible();
	const sakamoto = cards.filter({ hasText: 'Sakamoto Days' });
	// Covers are asked for only on screen, and on a phone this card can start below the fold.
	await sakamoto.scrollIntoViewIfNeeded();
	// Once asked for, its image is dropped when the request fails.
	await expect
		.poll(() => requested.some((url) => url.searchParams.get('link') === '/title/sd/sakamoto-days'))
		.toBe(true);
	await expect(sakamoto.locator('img')).toHaveCount(0);
	await expect(sakamoto.locator('.blank')).toHaveText('Sakamoto Days');
	// Both endpoints: Frieren's stored cover through the proxy, the others' series covers.
	const paths = new Set(requested.map((url) => url.pathname));
	expect(paths).toEqual(new Set(['/api/covers', '/api/covers/series']));
	for (const url of requested) expect(url.searchParams.get('w')).toBe('300');
});

test('a favorite’s missing cover shows once its series page has loaded one', async ({ page }) => {
	const requested: URL[] = [];
	await page.route(
		(url) => url.pathname.startsWith('/api/covers'),
		(route) => {
			const url = new URL(route.request().url());
			requested.push(url);
			// The series cover was stored as "no cover" while the website was blocking the server.
			return url.searchParams.get('link') === '/title/sd/sakamoto-days'
				? route.fulfill({ status: 404 })
				: route.fulfill({ contentType: 'image/png', body: PIXEL });
		}
	);
	await page.goto('/');
	const sakamoto = page
		.getByRole('list', { name: 'Favorites' })
		.getByRole('link')
		.filter({ hasText: 'Sakamoto Days' });
	await sakamoto.scrollIntoViewIfNeeded();
	await expect(sakamoto.locator('.blank')).toHaveText('Sakamoto Days');
	await expect(sakamoto.locator('img')).toHaveCount(0);

	await sakamoto.click();
	await expect(page.getByRole('heading', { name: 'Sakamoto Days' })).toBeVisible();
	// Back in the app, without a reload (which would reset the mock backend).
	await page.goBack();

	await sakamoto.scrollIntoViewIfNeeded();
	await expect(sakamoto.locator('img.loaded')).toBeVisible();
	expect(
		requested.some(
			(url) =>
				url.pathname === '/api/covers' &&
				url.searchParams.get('url') === 'https://mangadex.test/sakamoto-days.jpg'
		)
	).toBe(true);
});

test('Check now shows the check’s progress and the new chapters it found', async ({ page }) => {
	await page.goto('/');
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	await expect(cards).toHaveCount(8);

	await page.getByRole('button', { name: 'Check now' }).click();

	await expect(page.getByRole('progressbar', { name: 'Check progress' })).toBeVisible();
	// Seven are enabled; the failing Apothecary Diaries waits a day between checks.
	await expect(page.getByRole('status')).toContainText(/Checking \d+\/6/);
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

test('the library shows the titles picked in Settings, and sorts and searches by them', async ({
	page
}) => {
	await page.goto('/settings#section-appearance');
	await page.getByRole('combobox', { name: 'Library titles' }).selectOption({ label: 'Native' });
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');

	// Back in the Library without reloading the app.
	await page.getByRole('link', { name: 'Library' }).first().click();
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	await expect(cards).toHaveCount(8);
	const frieren = cards.locator('.t', { hasText: '葬送のフリーレン' });
	await expect(frieren).toHaveAttribute('title', 'Frieren');
	// No native title: MangaBaka's main title shows instead.
	await expect(cards.locator('.t', { hasText: 'Omniscient Reader' })).toHaveText(
		'Omniscient Reader'
	);
	// Not matched: the website's title, without a tooltip.
	await expect(cards.locator('.t', { hasText: 'Sakamoto Days' })).not.toHaveAttribute('title');

	await page.getByRole('combobox', { name: 'Sort' }).selectOption({ label: 'Title' });
	await expect(cards.last()).toContainText('葬送のフリーレン');

	const search = page.getByRole('searchbox', { name: 'Search the library' });
	await search.fill('フリーレン');
	await expect(cards).toHaveCount(1);
	await search.fill('frieren');
	await expect(cards).toHaveCount(1);
	await expect(cards.first()).toContainText('葬送のフリーレン');
});

test('series put in a group show in its section, which stays folded after a reload', async ({
	page
}) => {
	await page.goto('/');
	await page.getByRole('button', { name: 'Groups…' }).click();
	const editor = page.getByRole('dialog', { name: 'Groups' });
	await editor.getByRole('textbox', { name: 'New group' }).fill('Shounen');
	await editor.getByRole('button', { name: 'Add' }).click();
	await expect(editor.getByRole('list', { name: 'Groups' })).toContainText('Shounen · 0');
	await editor.getByRole('button', { name: 'Close' }).click();

	for (const title of ['Frieren', 'Sakamoto Days']) {
		await page.getByRole('link', { name: new RegExp(title) }).click();
		await page.getByRole('combobox', { name: 'Group' }).selectOption({ label: 'Shounen' });
		await expect(page.getByRole('combobox', { name: 'Group' })).toHaveValue(/\d+/);
		await page.getByRole('link', { name: '← Library' }).click();
	}

	const shounen = page.getByRole('list', { name: 'Shounen' }).getByRole('link');
	await expect(shounen).toHaveCount(2);
	await expect(shounen.filter({ hasText: 'Frieren' })).toHaveCount(1);
	const ungrouped = page.getByRole('list', { name: 'Ungrouped' }).getByRole('link');
	await expect(ungrouped).toHaveCount(6);
	// A paused series is marked so in its section too.
	await expect(ungrouped.filter({ hasText: 'Blue Lock' })).toContainText('Paused');

	const header = page.getByRole('button', { name: 'Shounen · 2' });
	await expect(header).toHaveAttribute('aria-expanded', 'true');
	await header.click();
	await expect(header).toHaveAttribute('aria-expanded', 'false');
	await expect(shounen).toHaveCount(0);

	await page.reload();
	await expect(page.getByRole('button', { name: 'Shounen · 2' })).toHaveAttribute(
		'aria-expanded',
		'false'
	);
	await expect(page.getByRole('list', { name: 'Ungrouped' }).getByRole('link')).toHaveCount(6);
	await expect(shounen).toHaveCount(0);
});

test('groups with a format or a tag rule collect the matching series without a group', async ({
	page
}) => {
	await page.goto('/');
	await page.getByRole('button', { name: 'Groups…' }).click();
	const editor = page.getByRole('dialog', { name: 'Groups' });
	const name = editor.getByRole('textbox', { name: 'New group' });
	const rule = editor.getByRole('combobox', { name: 'Auto-add' });

	await name.fill('Webtoons');
	await rule.selectOption({ label: 'Format' });
	await editor.getByRole('combobox', { name: 'Format' }).selectOption({ label: 'Manhwa' });
	await editor.getByRole('button', { name: 'Add' }).click();
	const groups = editor.getByRole('list', { name: 'Groups' });
	await expect(groups).toContainText('Webtoons · 1 · auto: Manhwa');

	await name.fill('Fights');
	await rule.selectOption({ label: 'Tag' });
	await editor.getByRole('textbox', { name: 'Tag' }).fill('action');
	await editor.getByRole('button', { name: 'Add' }).click();
	await expect(groups).toContainText('Fights · 1 · auto: tag action');
	await editor.getByRole('button', { name: 'Close' }).click();

	// Omniscient Reader is a manhwa tagged Action: the first group in order gets it.
	const webtoons = page.getByRole('list', { name: 'Webtoons' }).getByRole('link');
	await expect(webtoons).toHaveCount(1);
	await expect(webtoons).toContainText('Omniscient Reader');
	const fights = page.getByRole('list', { name: 'Fights' }).getByRole('link');
	await expect(fights).toHaveCount(1);
	await expect(fights).toContainText('Kagurabachi');
	await expect(page.getByRole('list', { name: 'Ungrouped' }).getByRole('link')).toHaveCount(6);
});

/** Fails unless `control`'s focus outline, as drawn, fits inside every ancestor that clips it. */
async function expectWholeOutline(control: Locator) {
	await expect(control).toBeFocused();
	const clipped = await control.evaluate((el) => {
		const style = getComputedStyle(el);
		const ring = parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset);
		if (style.outlineStyle === 'none' || !(ring > 0)) return 'no outline';
		const box = el.getBoundingClientRect();
		for (let ancestor = el.parentElement; ancestor; ancestor = ancestor.parentElement) {
			const overflow = getComputedStyle(ancestor);
			if (overflow.overflowX === 'visible' && overflow.overflowY === 'visible') continue;
			// The padding box, at fractional pixels: borders and a scrollbar don't show content.
			const rect = ancestor.getBoundingClientRect();
			const left = rect.left + ancestor.clientLeft;
			const top = rect.top + ancestor.clientTop;
			const right =
				rect.right - (ancestor.offsetWidth - ancestor.clientWidth - ancestor.clientLeft);
			const bottom =
				rect.bottom - (ancestor.offsetHeight - ancestor.clientHeight - ancestor.clientTop);
			if (
				box.left - ring < left ||
				box.top - ring < top ||
				box.right + ring > right ||
				box.bottom + ring > bottom
			)
				return `clipped by <${ancestor.tagName.toLowerCase()} class="${ancestor.className}">`;
		}
		return null;
	});
	expect(clipped, 'the whole focus outline shows').toBeNull();
}

/** Opens the Groups dialog and adds `names` from its "New group" box; the dialog. */
async function openGroups(page: Page, names: string[]) {
	await page.getByRole('button', { name: 'Groups…' }).click();
	const editor = page.getByRole('dialog', { name: 'Groups' });
	const name = editor.getByRole('textbox', { name: 'New group' });
	for (const group of names) {
		await name.fill(group);
		await name.press('Enter');
		await expect(editor.getByRole('list', { name: 'Groups' })).toContainText(`${group} · 0`);
	}
	return editor;
}

for (const [style, focusWidth] of [
	['default', '2px'],
	['high-contrast', '3px']
] as const) {
	test(`the Groups dialog shows the whole focus outline of its controls (${style})`, async ({
		page
	}) => {
		await page.goto('/');
		const editor = await openGroups(page, ['Shounen', 'Seinen']);
		// Shown without saving, once the app has applied the saved settings.
		await page.evaluate((s) => document.documentElement.setAttribute('data-style', s), style);
		const name = editor.getByRole('textbox', { name: 'New group' });
		await name.focus();
		await expect(name).toHaveCSS('outline-width', focusWidth);
		await expectWholeOutline(name);
		await name.fill('Josei');
		await page.keyboard.press('Tab');
		await expectWholeOutline(editor.getByRole('combobox', { name: 'Auto-add' }));
		await page.keyboard.press('Tab');
		await expectWholeOutline(editor.getByRole('button', { name: 'Add' }));

		for (const button of [
			'Move Seinen up',
			'Move Shounen down',
			'Edit Shounen',
			'Delete Shounen'
		]) {
			const control = editor.getByRole('button', { name: button });
			await control.focus();
			await expectWholeOutline(control);
		}

		// The controls of the edit and delete rows, reached from the keyboard.
		await editor.getByRole('button', { name: 'Edit Shounen' }).press('Enter');
		const rename = editor.getByRole('textbox', { name: 'Name of Shounen' });
		await rename.focus();
		await expectWholeOutline(rename);
		await page.keyboard.press('Tab');
		await expectWholeOutline(editor.getByRole('combobox', { name: 'Auto-add of Shounen' }));
		for (const button of ['Save', 'Cancel']) {
			await page.keyboard.press('Tab');
			await expectWholeOutline(editor.getByRole('button', { name: button, exact: true }));
		}
		await page.keyboard.press('Enter');

		await editor.getByRole('button', { name: 'Delete Shounen' }).press('Enter');
		for (const button of ['Delete', 'Cancel']) {
			const control = editor.getByRole('button', { name: button, exact: true });
			await control.focus();
			await expectWholeOutline(control);
		}
	});
}

test('a long group list scrolls inside the Groups dialog', async ({ page }) => {
	await page.goto('/');
	const editor = await openGroups(
		page,
		Array.from({ length: 40 }, (_, i) => `Group ${i + 1}`)
	);
	const groups = editor.getByRole('list', { name: 'Groups' });

	// The list's container scrolls, not the dialog, which stays within the window.
	const scrolls = (el: Element) => el.scrollHeight > el.clientHeight;
	expect(await groups.locator('..').evaluate(scrolls)).toBe(true);
	expect(await editor.evaluate(scrolls)).toBe(false);
	const box = await editor.boundingBox();
	expect((box?.y ?? Infinity) + (box?.height ?? 0)).toBeLessThanOrEqual(
		page.viewportSize()?.height ?? 0
	);

	// Focus scrolls a row into view, and its outline isn't cut off at the scrolling edge.
	const below = editor.getByRole('button', { name: 'Edit Group 30' });
	await below.focus();
	await expectWholeOutline(below);
	// Back to the top, so the next row scrolls in at the bottom edge.
	await editor.getByRole('button', { name: 'Edit Group 1', exact: true }).focus();
	const middle = editor.getByRole('button', { name: 'Edit Group 20' });
	await middle.focus();
	await expectWholeOutline(middle);
});

test('a series paused from its page shows as paused in the library, until resumed', async ({
	page
}) => {
	await page.goto('/');
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	const show = page.getByRole('group', { name: 'Show' });
	await expect(show.getByRole('button', { name: /^Paused/ })).toContainText('1');

	await cards.filter({ hasText: 'Frieren' }).click();
	await page.getByRole('button', { name: 'Pause checking' }).click();
	await expect(page.getByText('Paused: not checked for new chapters')).toBeVisible();
	await expect(page.getByRole('button', { name: 'Check missing chapters' })).toBeDisabled();

	await page.goBack();
	await expect(cards.filter({ hasText: 'Frieren' })).toContainText('Paused');
	await expect(show.getByRole('button', { name: /^Paused/ })).toContainText('2');
	await show.getByRole('button', { name: /^Paused/ }).click();
	await expect(cards.filter({ hasText: 'Frieren' })).toHaveCount(1);

	await cards.filter({ hasText: 'Frieren' }).click();
	await page.getByRole('button', { name: 'Resume checking' }).click();
	await expect(page.getByText('Paused: not checked for new chapters')).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Check missing chapters' })).toBeEnabled();

	await page.goBack();
	await expect(cards.filter({ hasText: 'Frieren' })).toContainText('MangaDex · Ongoing');
	await expect(show.getByRole('button', { name: /^Paused/ })).toContainText('1');
});

test.describe('selecting several series', () => {
	const THREE = ['Frieren', 'Kagurabachi', 'Sakamoto Days'];

	/** Turns on selection mode and selects `titles`; the cards, as checkboxes. */
	async function select(page: Page, titles: string[]) {
		await page.goto('/');
		await page.getByRole('button', { name: 'Select', exact: true }).click();
		for (const title of titles) {
			await page.getByRole('checkbox', { name: title }).click();
			await expect(page.getByRole('checkbox', { name: title })).toHaveAttribute(
				'aria-checked',
				'true'
			);
		}
		return page.getByRole('region', { name: 'Selected series' });
	}

	test('pauses them, then moves them to a new group', async ({ page }) => {
		const bar = await select(page, THREE);
		await expect(bar.getByRole('status')).toContainText('3 selected');

		await bar.getByRole('button', { name: 'Pause' }).click();
		for (const title of THREE) {
			await expect(page.getByRole('checkbox', { name: title })).toContainText('Paused');
		}
		await expect(page.getByRole('checkbox', { name: 'Vinland Saga' })).not.toContainText('Paused');
		// Paused ones are skipped by a check, and the bar says so.
		await expect(bar.getByRole('status')).toContainText('skips the paused');

		await bar.getByRole('button', { name: 'Group…' }).click();
		const dialog = page.getByRole('dialog', { name: 'Group' });
		await dialog.getByRole('combobox', { name: 'Group' }).selectOption({ label: 'New group…' });
		await dialog.getByRole('textbox', { name: 'New group name' }).fill('Weekly');
		await dialog.getByRole('button', { name: 'Save' }).click();
		await expect(dialog).toHaveCount(0);

		const weekly = page.getByRole('list', { name: 'Weekly' }).getByRole('checkbox');
		await expect(weekly).toHaveCount(3);
		await expect(weekly.filter({ hasText: 'Paused' })).toHaveCount(3);
		// The selection stays for the next action.
		await expect(bar.getByRole('status')).toContainText('3 selected');

		// A folded section's series are hidden: select all leaves them out of view and counts them.
		await page.getByRole('button', { name: 'Weekly · 3' }).click();
		await expect(bar.getByRole('status')).toContainText('3 selected (3 hidden)');
		await page.getByRole('button', { name: 'Select all', exact: true }).click();
		await expect(bar.getByRole('status')).toContainText('8 selected (3 hidden)');
		await page.getByRole('button', { name: 'Weekly · 3' }).click();

		await page.getByRole('button', { name: 'Done' }).click();
		await expect(bar).toHaveCount(0);
		await expect(page.getByRole('list', { name: 'Weekly' }).getByRole('link')).toHaveCount(3);
	});

	test('gives them a folder in a destination', async ({ page }) => {
		const bar = await select(page, THREE);

		await bar.getByRole('button', { name: 'Download folder…' }).click();
		const dialog = page.getByRole('dialog', { name: 'Download folder' });
		await expect(dialog).toContainText('not moved');
		await dialog.getByRole('combobox', { name: 'Destination' }).selectOption({
			label: 'Downloads (default)'
		});
		await dialog.getByRole('button', { name: 'Save' }).click();
		await expect(dialog).toHaveCount(0);

		await page.keyboard.press('Escape');
		await expect(bar).toHaveCount(0);
		await page.getByRole('link', { name: /Sakamoto Days/ }).click();
		await expect(page.getByRole('button', { name: 'Download folder…' })).toHaveAttribute(
			'title',
			'downloads/Sakamoto Days'
		);
	});

	test('marks them all seen, which clears their new-chapter badges', async ({ page }) => {
		const bar = await select(page, ['Frieren', 'Kagurabachi']);
		const badge = (title: string) =>
			page.getByRole('checkbox', { name: title }).getByLabel(/new chapters$/);
		await expect(badge('Frieren')).toHaveText('+4');
		await expect(badge('Kagurabachi')).toHaveText('+2');

		await bar.getByRole('button', { name: 'Mark all seen' }).click();

		await expect(badge('Frieren')).toHaveCount(0);
		await expect(badge('Kagurabachi')).toHaveCount(0);
		await expect(badge('Omniscient Reader’s Viewpoint')).toHaveText('+3');
	});

	test('removes one after confirming', async ({ page }) => {
		const bar = await select(page, ['Chainsaw Man']);

		await bar.getByRole('button', { name: 'Remove…' }).click();
		await expect(bar).toContainText(
			'Remove 1 series from the library? Downloaded chapters stay recorded.'
		);
		await bar.getByRole('button', { name: 'Remove', exact: true }).click();

		await expect(page.getByRole('checkbox', { name: 'Chainsaw Man' })).toHaveCount(0);
		await expect(page.getByRole('list', { name: 'Favorites' }).getByRole('checkbox')).toHaveCount(
			7
		);
		await expect(bar).toHaveCount(0);
	});

	test('select all takes the shown series, and keeps hidden ones selected', async ({ page }) => {
		const bar = await select(page, ['Frieren']);
		await page
			.getByRole('group', { name: 'Show' })
			.getByRole('button', { name: /^Completed/ })
			.click();
		await page.getByRole('button', { name: 'Select all' }).click();

		await expect(bar.getByRole('status')).toContainText('2 selected (1 hidden)');
	});
});

test('a series whose website keeps failing is marked, and its page offers another website', async ({
	page
}) => {
	await page.goto('/');
	const cards = page.getByRole('list', { name: 'Favorites' }).getByRole('link');
	const gone = cards.filter({ hasText: 'The Apothecary Diaries' });
	await expect(gone).toContainText('Not found');
	await expect(cards.filter({ hasText: 'Frieren' })).not.toContainText('Not found');

	await page
		.getByRole('group', { name: 'Show' })
		.getByRole('button', { name: /^Failing/ })
		.click();
	await expect(cards).toHaveCount(1);

	await gone.click();
	await expect(page.getByRole('alert').first()).toContainText(
		"The website hasn't answered for this series in its last 4 checks: HTTP 404 Not Found"
	);
	await expect(page.getByRole('button', { name: 'Check again' })).toBeVisible();
	await page.getByRole('link', { name: 'Find on another website' }).click();
	await expect(page).toHaveURL('/discover?q=The+Apothecary+Diaries');
	await expect(page.getByRole('searchbox', { name: 'Search titles' })).toHaveValue(
		'The Apothecary Diaries'
	);
});
