import { expect, test } from '@playwright/test';

test('a module option change is saved and still there after a reload', async ({ page }) => {
	await page.goto('/settings');
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

	await page.reload();
	await expect(modules.getByRole('combobox', { name: 'Language' })).toHaveValue('2');
	await expect(modules.getByRole('checkbox', { name: 'Data saver' })).toBeChecked();
	await expect(modules.getByRole('checkbox', { name: 'Show chapter title' })).toBeChecked();
});

test('an invalid setting shows the server error next to it', async ({ page }) => {
	await page.goto('/settings');
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
	await page.goto('/settings');
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
});

test('the table of contents is a sticky list on a wide screen', async ({ page }) => {
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto('/settings');
	const nav = page.getByRole('navigation', { name: 'Settings sections' });
	await expect(nav.getByRole('combobox', { name: 'Jump to section' })).toBeHidden();

	await nav.getByRole('link', { name: 'Website modules' }).click();
	await expect(page.getByRole('heading', { name: 'Website modules' })).toBeInViewport();
	await expect(nav).toBeInViewport();
});
