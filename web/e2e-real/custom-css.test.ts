import { expect, test, type Page } from '@playwright/test';

import { SITE } from './site.ts';

const PASSWORD = 'custom-css-e2e';

/** The page's computed background colour. */
const background = (page: Page) =>
	page.evaluate(() => getComputedStyle(document.body).backgroundColor);

test.afterEach(async ({ request }) => {
	await request.delete(`${SITE}/custom-css`);
});

// docs/tickets/T91-custom-css.md: a `custom.css` in the data folder restyles the UI on the next
// load, without a restart, and the login page too.
test('a custom.css in the data folder restyles the app and the login page', async ({
	page,
	request
}) => {
	await page.goto('/');
	await expect(page.getByRole('textbox', { name: 'Manga URL' })).toBeVisible();
	const original = await background(page);

	await request.put(`${SITE}/custom-css`, { data: ':root { --bg: rgb(1, 2, 3); }' });
	await page.reload();
	await expect(page.getByRole('textbox', { name: 'Manga URL' })).toBeVisible();
	expect(await background(page)).toBe('rgb(1, 2, 3)');

	await request.delete(`${SITE}/custom-css`);
	await page.reload();
	await expect(page.getByRole('textbox', { name: 'Manga URL' })).toBeVisible();
	expect(await background(page)).toBe(original);

	// Behind a password the login page loads it too.
	await request.put(`${SITE}/custom-css`, { data: ':root { --bg: rgb(4, 5, 6); }' });
	expect(
		(await request.patch('/api/settings', { data: { server: { auth_token: PASSWORD } } })).ok()
	).toBe(true);
	try {
		await page.context().clearCookies();
		await page.reload();
		await expect(page.getByRole('heading', { name: 'Log in' })).toBeVisible();
		expect(await background(page)).toBe('rgb(4, 5, 6)');
	} finally {
		// The other tests share the server: clear the password again.
		expect((await request.post('/api/login', { data: { password: PASSWORD } })).ok()).toBe(true);
		expect(
			(await request.patch('/api/settings', { data: { server: { auth_token: '' } } })).ok()
		).toBe(true);
	}
});

test('Settings → General says where custom.css goes', async ({ page }) => {
	await page.goto('/settings#section-general');
	await expect(
		page.getByText(/custom\.css in the data folder \(\S*fmd2r-e2e-\S*\/data\) is loaded/)
	).toBeVisible();
});
