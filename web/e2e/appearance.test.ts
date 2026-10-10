import { expect, test, type Page } from '@playwright/test';

/** Background of the page in each mode (`--bg` in tokens.css). */
const LIGHT_BG = 'rgb(243, 245, 246)';
const DARK_BG = 'rgb(16, 22, 24)';
/** The purple swatch's dark `--accent`. */
const DARK_PURPLE = 'rgb(181, 156, 242)';

/** Opens Settings → Appearance. */
async function openAppearance(page: Page) {
	await page.goto('/settings#section-appearance');
	await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
}

async function save(page: Page) {
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');
}

/** What the page shows now: its background, `--fs-md` and `--accent` as computed values. */
function look(page: Page) {
	return page.evaluate(() => {
		const probe = document.createElement('div');
		probe.style.fontSize = 'var(--fs-md)';
		probe.style.color = 'var(--accent)';
		document.body.append(probe);
		const style = getComputedStyle(probe);
		const result = {
			background: getComputedStyle(document.body).backgroundColor,
			fontSize: style.fontSize,
			accent: style.color
		};
		probe.remove();
		return result;
	});
}

test('an appearance shows as soon as it is picked, and from the first frame after a reload', async ({
	page
}) => {
	await openAppearance(page);
	expect(await look(page)).toMatchObject({ background: LIGHT_BG, fontSize: '14px' });

	await page.getByRole('combobox', { name: 'Light or dark' }).selectOption({ label: 'Dark' });
	await page.getByRole('combobox', { name: 'Text size' }).selectOption({ label: 'Large' });
	await page.getByRole('radio', { name: 'Purple' }).click();
	// Previewed before saving.
	expect(await look(page)).toEqual({
		background: DARK_BG,
		fontSize: '16.1px',
		accent: DARK_PURPLE
	});
	await save(page);

	// Record the background as the document is parsed, before the settings could answer.
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
	expect(first).toBe(DARK_BG);
	await expect(page.getByRole('heading', { name: 'Appearance' })).toBeVisible();
	expect(await look(page)).toEqual({
		background: DARK_BG,
		fontSize: '16.1px',
		accent: DARK_PURPLE
	});
});

test('discarding an unsaved appearance shows the saved one again', async ({ page }) => {
	await openAppearance(page);
	await page.getByRole('combobox', { name: 'Light or dark' }).selectOption({ label: 'Dark' });
	expect((await look(page)).background).toBe(DARK_BG);

	await page
		.getByRole('region', { name: 'Save changes' })
		.getByRole('button', { name: 'Discard' })
		.click();
	expect((await look(page)).background).toBe(LIGHT_BG);
});

test('the largest text still fits every page without scrolling sideways', async ({ page }) => {
	await openAppearance(page);
	await page.getByRole('combobox', { name: 'Text size' }).selectOption({ label: 'Larger' });
	await save(page);

	const pages: [string, RegExp | string][] = [
		['/', 'Library'],
		['/discover', 'Discover'],
		['/queue', 'Queue'],
		['/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren', 'Frieren'],
		['/settings', 'Settings'],
		['/system', 'System']
	];
	for (const [url, heading] of pages) {
		await page.goto(url);
		await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
		expect((await look(page)).fontSize).toBe('18.2px');
		const overflow = await page.evaluate(
			() => document.documentElement.scrollWidth - window.innerWidth
		);
		expect(overflow, url).toBeLessThanOrEqual(0);
		// No button or navigation label runs past its box. A badge placed over the edge on purpose
		// (positioned absolutely, like the inbox count) isn't part of the label, and text cut short
		// with an ellipsis (a log line) is meant to be.
		const clipped = await page.evaluate(() =>
			[...document.querySelectorAll<HTMLElement>('button, nav a')]
				.filter((el) => el.offsetParent !== null)
				.filter((el) => {
					const box = el.getBoundingClientRect();
					const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
					for (let node = walker.nextNode(); node; node = walker.nextNode()) {
						const parent = node.parentElement;
						if (!node.textContent?.trim() || !parent) continue;
						if (parent !== el && parent.closest('button, nav a') !== el) continue;
						const style = getComputedStyle(parent);
						if (['absolute', 'fixed'].includes(style.position)) continue;
						const ellipsis = (e: Element | null) =>
							!!e && getComputedStyle(e).textOverflow === 'ellipsis';
						if (ellipsis(parent) || ellipsis(parent.parentElement)) continue;
						const range = document.createRange();
						range.selectNodeContents(node);
						const text = range.getBoundingClientRect();
						// Not rendered (inside an element that is hidden).
						if (!text.width && !text.height) continue;
						if (
							text.left < box.left - 1 ||
							text.right > box.right + 1 ||
							text.top < box.top - 1 ||
							text.bottom > box.bottom + 1
						)
							return true;
					}
					return false;
				})
				.map((el) => el.textContent?.trim() || el.getAttribute('aria-label'))
		);
		expect(clipped, url).toEqual([]);
	}
});
