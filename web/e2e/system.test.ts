import { expect, test } from '@playwright/test';

test('system logs stream in and can be filtered', async ({ page }) => {
	await page.goto('/system');
	const log = page.getByRole('log', { name: 'Server log' });
	await expect(log).toContainText('favorites check started');
	// The mock saves a page of each downloading task once a second; follow mode keeps it in view.
	await expect(log).toContainText('Kagurabachi: page 13/19 saved');

	await page.getByRole('combobox', { name: 'Level' }).selectOption('ERROR');
	await expect(log).toContainText('ResolveRedirect');
	await expect(log).not.toContainText('page 2');

	await page.getByRole('combobox', { name: 'Level' }).selectOption('TRACE');
	await page.getByRole('searchbox', { name: 'Search logs' }).fill('rate limited');
	await expect(log).toContainText('rate limited, retrying');
	await expect(log).not.toContainText('favorites check started');
});

test('pausing the log holds new lines back until resumed', async ({ page }) => {
	await page.goto('/system');
	const log = page.getByRole('log', { name: 'Server log' });
	await expect(log).toContainText('favorites check started');

	await page.getByRole('button', { name: 'Pause' }).click();
	await expect(page.getByRole('button', { name: /Resume \(\d+ new\)/ })).toBeVisible();
	await expect(log).not.toContainText('Kagurabachi: page 14/19 saved');

	await page.getByRole('button', { name: /Resume/ }).click();
	await expect(log).toContainText('Kagurabachi: page 14/19 saved');
});

test('running a job shows its progress', async ({ page }) => {
	await page.goto('/system');
	await page.getByRole('tab', { name: 'Jobs' }).click();
	const card = page.getByRole('article', { name: 'Update lists' });
	await expect(card).toContainText('Idle');

	await card.getByRole('button', { name: 'Run' }).click();
	await expect(card).toContainText('Running');
	await expect(card.getByRole('button', { name: 'Run' })).toBeDisabled();
	const bar = card.getByRole('progressbar');
	await expect(bar).toBeVisible();
	await expect(bar).not.toHaveAttribute('aria-valuenow', '0');

	await card.getByRole('button', { name: 'Cancel' }).click();
	await expect(card).toContainText('Idle');
});

test('a failed job shows its last error on request', async ({ page }) => {
	await page.goto('/system');
	await page.getByRole('tab', { name: 'Jobs' }).click();
	const card = page.getByRole('article', { name: 'Update modules' });
	await expect(card).toContainText('Failed');
	await card.getByText('Last error').click();
	await expect(card).toContainText('rate limit exceeded');
});

test('about lists diagnostics and highlights failed tool checks', async ({ page }) => {
	await page.goto('/system');
	await page.getByRole('tab', { name: 'About' }).click();
	const about = page.getByRole('tabpanel', { name: 'About' });
	await expect(about).toContainText('0.1.0');
	await expect(about).toContainText('master');
	await expect(about).toContainText('665');
	await expect(about).toContainText('/data');

	const magick = about.getByRole('row', { name: /magick/ });
	await expect(magick).toContainText('Missing');
	await expect(magick).toHaveClass(/failed/);
	await expect(about.getByRole('row', { name: /python3/ })).not.toHaveClass(/failed/);

	await about.getByRole('button', { name: 'Show in inbox' }).click();
	const inbox = page.getByRole('dialog', { name: 'Inbox' });
	await expect(inbox).toBeVisible();
	await expect(inbox.locator('.focused')).toContainText('Bato.to module needs a newer FMD2r');
});
