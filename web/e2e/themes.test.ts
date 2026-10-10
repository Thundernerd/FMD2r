import { expect, test, type Page } from '@playwright/test';

/** The page's background in light mode per theme (`--bg` in tokens.css). */
const BACKGROUND = {
	Default: 'rgb(243, 245, 246)',
	'High contrast': 'rgb(244, 244, 244)',
	Warm: 'rgb(244, 238, 226)',
	Compact: 'rgb(236, 240, 241)'
};
/** High contrast's own dark background. */
const HIGH_CONTRAST_DARK_BG = 'rgb(0, 0, 0)';

async function openAppearance(page: Page) {
	await page.goto('/settings#section-appearance');
	await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
}

async function pickTheme(page: Page, name: string) {
	await page.getByRole('radiogroup', { name: 'Theme' }).getByRole('radio', { name }).click();
}

async function save(page: Page) {
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');
}

const background = (page: Page) =>
	page.evaluate(() => getComputedStyle(document.body).backgroundColor);

test('each theme shows as soon as it is picked', async ({ page }) => {
	await openAppearance(page);
	await page.getByRole('combobox', { name: 'Light or dark' }).selectOption({ label: 'Light' });
	expect(await background(page)).toBe(BACKGROUND.Default);
	for (const [name, bg] of Object.entries(BACKGROUND).reverse()) {
		await pickTheme(page, name);
		expect(await background(page), name).toBe(bg);
	}
});

test('high contrast keeps its own accent and greys out the swatches', async ({ page }) => {
	await openAppearance(page);
	const purple = page.getByRole('radio', { name: 'Purple' });
	await expect(purple).toBeEnabled();
	await pickTheme(page, 'High contrast');
	await expect(purple).toBeDisabled();
	await expect(page.getByText('High contrast keeps its own accent')).toBeVisible();
	await pickTheme(page, 'Warm');
	await expect(purple).toBeEnabled();
});

test('a theme shows from the first frame after a reload, in dark too', async ({ page }) => {
	await openAppearance(page);
	await page.getByRole('combobox', { name: 'Light or dark' }).selectOption({ label: 'Dark' });
	await pickTheme(page, 'High contrast');
	expect(await background(page)).toBe(HIGH_CONTRAST_DARK_BG);
	await save(page);

	await page.addInitScript(() => {
		document.addEventListener('DOMContentLoaded', () => {
			(window as unknown as { firstBackground: string }).firstBackground = getComputedStyle(
				document.body
			).backgroundColor;
		});
	});
	await page.reload();
	const first = await page.evaluate(
		() => (window as unknown as { firstBackground: string }).firstBackground
	);
	expect(first).toBe(HIGH_CONTRAST_DARK_BG);
	await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
	expect(await background(page)).toBe(HIGH_CONTRAST_DARK_BG);
	await expect(
		page.getByRole('radiogroup', { name: 'Theme' }).getByRole('radio', { name: 'High contrast' })
	).toBeChecked();
});

for (const theme of ['Compact', 'Warm']) {
	test(`${theme} fits Library and Discover at 375px without scrolling sideways`, async ({
		page
	}) => {
		await page.setViewportSize({ width: 375, height: 740 });
		await openAppearance(page);
		await pickTheme(page, theme);
		await save(page);
		const pages: [string, string][] = [
			['/', 'Library'],
			['/discover', 'Discover']
		];
		for (const [url, heading] of pages) {
			await page.goto(url);
			await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
			expect(await page.evaluate(() => document.documentElement.dataset['style'])).toBe(
				theme.toLowerCase()
			);
			const overflow = await page.evaluate(
				() => document.documentElement.scrollWidth - window.innerWidth
			);
			expect(overflow, url).toBeLessThanOrEqual(0);
		}
	});
}
