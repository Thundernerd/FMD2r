import { expect, test } from '@playwright/test';

// Issue #311: "Add to Home Screen" installs FMD2r as a standalone app from its web app manifest.
test('the page links a web app manifest for a standalone app with 192 and 512 icons', async ({
	page,
	request
}) => {
	await page.goto('/');
	const href = await page.locator('link[rel="manifest"]').getAttribute('href');
	expect(href).toBeTruthy();

	const res = await request.get(new URL(href!, page.url()).href);
	expect(res.ok()).toBe(true);
	const manifest = JSON.parse(await res.text());

	expect(manifest.display).toBe('standalone');
	const sizes = manifest.icons.map((icon: { sizes: string }) => icon.sizes);
	expect(sizes).toContain('192x192');
	expect(sizes).toContain('512x512');
});
