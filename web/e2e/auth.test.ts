import { expect, test, type Page } from '@playwright/test';

/** Makes the mock backend require `password`, like `fmd2r serve --password`. */
const requirePassword = (page: Page, password: string) =>
	page.addInitScript((pw) => sessionStorage.setItem('fmd2r.mock.password', pw), password);

test('a 401 shows the login screen, and logging in opens the app', async ({ page }) => {
	await requirePassword(page, 'hunter2');
	await page.goto('/queue');

	const login = page.getByRole('form', { name: 'Log in' });
	await expect(login).toBeVisible();
	await expect(page.getByRole('heading', { level: 1, name: 'Queue' })).toHaveCount(0);

	await login.getByLabel('Password').fill('wrong');
	await login.getByRole('button', { name: 'Log in' }).click();
	await expect(login.getByRole('alert')).toHaveText('Wrong password.');

	await login.getByLabel('Password').fill('hunter2');
	await login.getByRole('button', { name: 'Log in' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Queue' })).toBeVisible();
	await expect(page.getByRole('main')).toContainText('Kagurabachi');

	// The session outlives a reload, like the cookie.
	await page.reload();
	await expect(page.getByRole('heading', { level: 1, name: 'Queue' })).toBeVisible();
});

test('logging out returns to the login screen', async ({ page }) => {
	await requirePassword(page, 'hunter2');
	await page.goto('/');
	const login = page.getByRole('form', { name: 'Log in' });
	await login.getByLabel('Password').fill('hunter2');
	await login.getByRole('button', { name: 'Log in' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();

	await page.getByRole('button', { name: 'Log out' }).click();
	await expect(login).toBeVisible();
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toHaveCount(0);

	await page.reload();
	await expect(login).toBeVisible();
});

test('without a password there is no login screen and no log out', async ({ page }) => {
	await page.goto('/');
	await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
	await expect(page.getByRole('form', { name: 'Log in' })).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Log out' })).toHaveCount(0);
});
