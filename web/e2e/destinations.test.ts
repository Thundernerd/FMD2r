import { expect, test } from '@playwright/test';

test('a chapter downloads to a second destination added in Settings', async ({ page }) => {
	await page.goto('/settings#section-saveto');
	const section = page.getByRole('region', { name: 'Save to' });
	await section.getByRole('button', { name: 'Add destination' }).click();
	const added = section.getByRole('group', { name: 'New destination' });
	await added.getByRole('textbox', { name: 'Name' }).fill('Manhwa');
	const manhwa = section.getByRole('group', { name: 'Manhwa' });
	await manhwa.getByRole('textbox', { name: 'Folder' }).fill('/data/manhwa');
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');

	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');
	const box = page.getByRole('region', { name: 'Download', exact: true });
	const picker = box.getByRole('combobox', { name: 'Save to' });
	await expect(picker).toHaveValue('0');
	await picker.selectOption({ label: 'Manhwa' });
	await expect(box).toContainText('/data/manhwa/Frieren');
	const chapters = page.getByRole('region', { name: 'Chapters' });
	await chapters.getByRole('checkbox', { name: 'Chapter 1', exact: true }).check();
	await box.getByRole('button', { name: 'Download 1 chapter' }).click();
	await expect(box.getByRole('status')).toContainText('Queued: Frieren, 1 chapter');

	// In the app, not by reload: the mock keeps its tasks in memory.
	await box.getByRole('link', { name: 'Open queue' }).click();
	const task = page.getByRole('listitem', { name: 'Frieren' }).filter({ hasText: '/data/manhwa' });
	await expect(task).toContainText('/data/manhwa/Frieren');
});

test('a missing destination folder gets a warning, not an error', async ({ page }) => {
	await page.goto('/settings#section-saveto');
	const section = page.getByRole('region', { name: 'Save to' });
	await section.getByRole('button', { name: 'Add destination' }).click();
	const added = section.getByRole('group', { name: 'New destination' });
	await added.getByRole('textbox', { name: 'Folder' }).fill('/mnt/usb');
	await expect(added).toContainText('the folder does not exist');
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');
});

test("a library series' download folder is changed from its series page", async ({ page }) => {
	await page.goto('/settings#section-saveto');
	const section = page.getByRole('region', { name: 'Save to' });
	await section.getByRole('button', { name: 'Add destination' }).click();
	const added = section.getByRole('group', { name: 'New destination' });
	await added.getByRole('textbox', { name: 'Folder' }).fill('/data/manhwa');
	const bar = page.getByRole('region', { name: 'Save changes' });
	await bar.getByRole('button', { name: 'Save' }).click();
	await expect(bar).toContainText('Saved');

	// Frieren is in the library.
	await page.goto('/series?module=mangadex&link=%2Ftitle%2Fabc123%2Ffrieren');
	const button = page.getByRole('button', { name: 'Download folder…' });
	await button.click();
	const dialog = page.getByRole('dialog', { name: 'Download folder' });
	await expect(dialog).toContainText('they are not moved');
	await expect(button).toHaveAttribute('title', '/data/downloads/Frieren');
	await dialog.getByRole('combobox', { name: 'Destination' }).selectOption({
		label: 'New destination'
	});
	await expect(dialog.getByRole('textbox', { name: 'Folder' })).toHaveValue('/data/manhwa/Frieren');
	await dialog.getByRole('button', { name: 'Save' }).click();
	await expect(dialog).toBeHidden();

	await button.click();
	await expect(dialog.getByRole('textbox', { name: 'Folder' })).toHaveValue('/data/manhwa/Frieren');
	await expect(button).toHaveAttribute('title', '/data/manhwa/Frieren');
});
