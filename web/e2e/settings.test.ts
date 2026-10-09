import { expect, test, type Page } from '@playwright/test';

/** Switches to a settings section through the table of contents: a list, or a phone's dropdown. */
async function openSection(page: Page, id: string, title: string) {
	const nav = page.getByRole('navigation', { name: 'Settings sections' });
	// It shows once the settings have loaded; only then is it known which of the two is visible.
	await nav.waitFor();
	const link = nav.getByRole('link', { name: title, exact: true });
	if (await link.isVisible()) await link.click();
	else await nav.getByRole('combobox', { name: 'Jump to section' }).selectOption(id);
}

test('a module option change is saved and still there after a reload', async ({ page }) => {
	await page.goto('/settings#section-modules');
	const modules = page.getByRole('region', { name: 'Website modules' });
	await modules.getByRole('searchbox', { name: 'Search modules' }).fill('dex');
	await modules.getByRole('button', { name: /MangaDex/ }).click();

	const language = modules.getByRole('combobox', { name: 'Language' });
	await expect(language).toHaveValue('1');
	await language.selectOption({ label: 'Japanese' });
	await modules.getByRole('checkbox', { name: 'Data saver' }).check();

	const bar = page.getByRole('region', { name: 'Save changes' });
	await expect(bar).toContainText('Unsaved changes');
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');

	// A link to a module without a section opens the module settings.
	const url = new URL(page.url());
	url.hash = '';
	await page.goto(url.href);
	await expect(modules.getByRole('combobox', { name: 'Language' })).toHaveValue('2');
	await expect(modules.getByRole('checkbox', { name: 'Data saver' })).toBeChecked();
	await expect(modules.getByRole('checkbox', { name: 'Show chapter title' })).toBeChecked();
});

test('an invalid setting shows the server error next to it', async ({ page }) => {
	await page.goto('/settings#section-connections');
	const parallel = page.getByRole('spinbutton', { name: 'Parallel downloads' });
	await parallel.fill('0');
	await page
		.getByRole('region', { name: 'Save changes' })
		.getByRole('button', { name: 'Save' })
		.click();

	await expect(parallel).toHaveAttribute('aria-invalid', 'true');
	await expect(parallel).toHaveAccessibleDescription(/0 is outside 1..=64/);

	await parallel.fill('3');
	await expect(parallel).not.toHaveAttribute('aria-invalid', 'true');
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');
	await page.reload();
	await expect(page.getByRole('spinbutton', { name: 'Parallel downloads' })).toHaveValue('3');
});

test('discarding drops unsaved edits', async ({ page }) => {
	await page.goto('/settings');
	const stopped = page.getByRole('checkbox', { name: 'Add new downloads stopped' });
	await stopped.check();
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Discard' }).click();
	await expect(stopped).not.toBeChecked();
	await expect(bar).toBeHidden();
});

test('the rename preview follows the templates as they are typed', async ({ page }) => {
	await page.goto('/settings#section-saveto');
	await page.getByRole('textbox', { name: 'Manga folder name' }).fill('%WEBSITE% - %MANGA%');
	await expect(page.getByText('downloads/MangaDex - Sample Manga/')).toBeVisible();
});

test('the table of contents collapses to a dropdown on a phone', async ({ page }) => {
	await page.setViewportSize({ width: 375, height: 740 });
	await page.goto('/settings');
	const nav = page.getByRole('navigation', { name: 'Settings sections' });
	await expect(nav.getByRole('link', { name: 'Output' })).toBeHidden();

	await nav.getByRole('combobox', { name: 'Jump to section' }).selectOption({ label: 'Output' });
	await expect(page.getByRole('heading', { name: 'Output' })).toBeInViewport();
	await expect(page.getByRole('heading', { name: 'General' })).toHaveCount(0);
	await expect(page).toHaveURL(/#section-output$/);
});

test('the table of contents is a sticky list on a wide screen', async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto('/settings');
	const nav = page.getByRole('navigation', { name: 'Settings sections' });
	await expect(nav.getByRole('combobox', { name: 'Jump to section' })).toBeHidden();

	await nav.getByRole('link', { name: 'Website modules' }).click();
	await expect(page.getByRole('heading', { name: 'Website modules' })).toBeInViewport();
	await expect(page.getByRole('heading', { name: 'General' })).toHaveCount(0);
	await expect(nav.getByRole('link', { name: 'Website modules' })).toHaveAttribute(
		'aria-current',
		'location'
	);
	await expect(nav).toBeInViewport();
});

test('a section has its own link, and Back returns to the one before', async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto('/settings#section-server');
	await expect(page.getByRole('heading', { name: 'Server' })).toBeVisible();
	await expect(page.getByRole('heading', { name: 'General' })).toHaveCount(0);

	const nav = page.getByRole('navigation', { name: 'Settings sections' });
	await nav.getByRole('link', { name: 'Output' }).click();
	await expect(page).toHaveURL(/#section-output$/);
	await page.goBack();
	await expect(page.getByRole('heading', { name: 'Server' })).toBeVisible();
	await expect(page.getByRole('heading', { name: 'Output' })).toHaveCount(0);
});

test('clearing a required number asks for one instead of resetting it', async ({ page }) => {
	await page.goto('/settings#section-connections');
	const timeout = page.getByRole('spinbutton', { name: 'Connection timeout (seconds)' });
	await timeout.fill('45');
	await timeout.fill('');

	await expect(timeout).toHaveAttribute('aria-invalid', 'true');
	await expect(timeout).toHaveAccessibleDescription(/Enter a number/);
	const bar = page.getByRole('region', { name: 'Save changes' });
	await expect(bar.getByRole('button', { name: 'Save' })).toBeDisabled();

	await timeout.fill('60');
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');
	await page.reload();
	await expect(page.getByRole('spinbutton', { name: 'Connection timeout (seconds)' })).toHaveValue(
		'60'
	);
});

test('every invalid setting of a save is shown next to its field at once', async ({ page }) => {
	// The marks on the table of contents show on the wide screen's list.
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto('/settings#section-modules');
	const modules = page.getByRole('region', { name: 'Website modules' });
	await modules.getByRole('searchbox', { name: 'Search modules' }).fill('dex');
	await modules.getByRole('button', { name: /MangaDex/ }).click();
	await modules.getByRole('spinbutton', { name: 'Delay (s) between requests' }).fill('10001');
	await openSection(page, 'connections', 'Connections');
	const parallel = page.getByRole('spinbutton', { name: 'Parallel downloads' });
	const timeout = page.getByRole('spinbutton', { name: 'Connection timeout (seconds)' });
	await parallel.fill('0');
	await timeout.fill('0');

	await page
		.getByRole('region', { name: 'Save changes' })
		.getByRole('button', { name: 'Save' })
		.click();

	await expect(parallel).toHaveAccessibleDescription(/0 is outside 1..=64/);
	await expect(timeout).toHaveAccessibleDescription(/0 is outside 1..=300/);
	const nav = page.getByRole('navigation', { name: 'Settings sections' });
	await expect(nav.getByRole('link', { name: 'Website modules' })).toHaveAccessibleDescription(
		'Invalid settings'
	);
	await nav.getByRole('link', { name: 'Website modules' }).click();
	await expect(
		modules.getByRole('spinbutton', { name: 'Delay (s) between requests' })
	).toHaveAccessibleDescription(/expected an integer in 0..=10000/);
});

test('a rejected module change does not save the other settings either', async ({ page }) => {
	await page.goto('/settings');
	await page.getByRole('checkbox', { name: 'Add new downloads stopped' }).check();
	await openSection(page, 'modules', 'Website modules');
	const modules = page.getByRole('region', { name: 'Website modules' });
	await modules.getByRole('searchbox', { name: 'Search modules' }).fill('dex');
	await modules.getByRole('button', { name: /MangaDex/ }).click();
	await modules.getByRole('spinbutton', { name: 'Delay (s) between requests' }).fill('10001');

	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Fix the highlighted settings to save');

	await page.reload();
	await openSection(page, 'general', 'General');
	await expect(page.getByRole('checkbox', { name: 'Add new downloads stopped' })).not.toBeChecked();
});

test('the rename preview follows the chapter settings', async ({ page }) => {
	await page.goto('/settings#section-saveto');
	const preview = page.locator('.preview');
	await expect(preview).toContainText('downloads/Sample Manga/Sample Manga - Vol. 1 Ch. 5/001.jpg');
	await page.getByRole('checkbox', { name: /Remove the manga name from chapter names/ }).check();
	await expect(preview).toContainText('downloads/Sample Manga/Vol. 1 Ch. 5/001.jpg');
	await page.getByRole('checkbox', { name: 'Create a folder per chapter' }).uncheck();
	await expect(preview).toContainText('downloads/Sample Manga/001.jpg');
});

test('opening a module by link selects it and shows it in the list', async ({ page }) => {
	await page.goto('/settings?module=tmo');
	const list = page
		.getByRole('region', { name: 'Website modules' })
		.getByRole('list', { name: 'Modules' });
	const pick = list.getByRole('button', { name: /TuMangaOnline/ });
	await expect(pick).toHaveAttribute('aria-pressed', 'true');
	await expect(pick).toBeInViewport();
});

test('the website selection shows its search and lines its websites up in columns', async ({
	page
}) => {
	await page.goto('/settings#section-websites');
	const websites = page.getByRole('region', { name: 'Websites', exact: true });
	const search = websites.getByRole('searchbox', { name: 'Search websites' });
	// As tall as a plain input, not cut to a progress bar's 6px.
	const normal = await page.evaluate(() => {
		const input = document.body.appendChild(document.createElement('input'));
		input.className = 'input';
		const height = input.getBoundingClientRect().height;
		input.remove();
		return height;
	});
	const shown = await search.evaluate((input) => {
		// The part of the box its row does not clip away.
		const box = input.getBoundingClientRect();
		const row = input.parentElement?.getBoundingClientRect() ?? box;
		return Math.min(box.bottom, row.bottom) - Math.max(box.top, row.top);
	});
	expect(shown).toBeGreaterThanOrEqual(normal);
	await expect(websites.getByRole('button', { name: 'Select all' })).toBeVisible();
	await expect(websites.getByRole('button', { name: 'Select none' })).toBeVisible();

	// A long name takes no more room than the others, so the websites line up in columns.
	const arabic = websites.getByRole('group', { name: 'Arabic' }).getByRole('checkbox');
	await expect(arabic).toHaveCount(3);
	const items = await arabic.evaluateAll((boxes) =>
		boxes.map((box) => {
			const choice = box.closest('label')?.getBoundingClientRect();
			return { top: choice?.top, left: box.getBoundingClientRect().left, width: choice?.width };
		})
	);
	expect(new Set(items.map((item) => item.width)).size).toBe(1);
	const columns = items.filter((item) => item.top === items[0]?.top).length;
	items.forEach((item, i) => expect(item.left).toBe(items[i % columns]?.left));
});
