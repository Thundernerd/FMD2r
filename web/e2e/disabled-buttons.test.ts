import { expect, test, type Locator, type Page, type TestInfo } from '@playwright/test';
import { USERDATA_ZIP } from './userdata.ts';

/** The themes whose disabled buttons are screenshot for review (issue #226). */
const THEMES = ['default', 'high-contrast', 'warm'] as const;
const MODES = ['light', 'dark'] as const;

/** A disabled button is faded and shows it won't respond, whatever its theme. */
async function expectFaded(button: Locator) {
	await expect(button).toBeDisabled();
	await expect(button).toHaveCSS('opacity', '0.45');
	await expect(button).toHaveCSS('cursor', 'not-allowed');
}

/** Hovering a disabled button leaves its border as it was. */
async function expectNoHighlight(button: Locator) {
	const before = await button.evaluate((el) => getComputedStyle(el).borderColor);
	await button.hover({ force: true });
	await expect(button).toHaveCSS('border-color', before);
}

/** Screenshots `area` in each theme and mode into `test-results/`, and the report, for review. */
async function screenshots(page: Page, area: Locator, name: string, testInfo: TestInfo) {
	for (const mode of MODES) {
		await page.emulateMedia({ colorScheme: mode });
		for (const theme of THEMES) {
			await page.evaluate((t) => document.documentElement.setAttribute('data-style', t), theme);
			const path = testInfo.outputPath(`${name}-${theme}-${mode}.png`);
			await area.screenshot({ path, animations: 'disabled' });
			await testInfo.attach(`${name}, ${theme}, ${mode}`, { path, contentType: 'image/png' });
		}
	}
}

test('the setup Import step greys out its buttons during an import', async ({ page }, testInfo) => {
	await page.addInitScript(() => {
		sessionStorage.setItem('fmd2r.mock.fresh-install', '1');
		// Long enough to look at every theme before the import finishes.
		sessionStorage.setItem('fmd2r.mock.import-step-ms', '60000');
	});
	await page.goto('/');
	await page.getByRole('button', { name: 'Next' }).click();
	const step = page.getByRole('region', { name: 'Import from FMD2' });
	await step.getByRole('button', { name: 'Import' }).click();
	await step.getByLabel('FMD2 userdata folder, zipped').setInputFiles(USERDATA_ZIP);
	await step.getByRole('button', { name: 'Check' }).click();

	await expect(step.getByRole('button', { name: 'Checking…' })).toBeVisible();
	const skip = page.getByRole('button', { name: 'Skip' });
	for (const theme of THEMES) {
		await page.evaluate((t) => document.documentElement.setAttribute('data-style', t), theme);
		await expectFaded(skip);
		await expectFaded(step.getByRole('button', { name: 'Import', exact: true }));
	}
	await expectNoHighlight(skip);
	await screenshots(page, page.locator('main'), 'setup-import-step-importing', testInfo);
});

test('the FMD2 import flow greys out Check and Import before a check', async ({
	page
}, testInfo) => {
	await page.goto('/');
	await page.getByRole('button', { name: 'Import from FMD2' }).click();
	const dialog = page.getByRole('dialog', { name: 'Import from FMD2' });
	const check = dialog.getByRole('button', { name: 'Check' });
	const importButton = dialog.getByRole('button', { name: 'Import', exact: true });

	for (const theme of THEMES) {
		await page.evaluate((t) => document.documentElement.setAttribute('data-style', t), theme);
		await expectFaded(check);
		await expectFaded(importButton);
	}
	await expectNoHighlight(check);
	await expectNoHighlight(importButton);
	await screenshots(page, dialog, 'fmd2-import-no-file', testInfo);

	await dialog.getByLabel('FMD2 userdata folder, zipped').setInputFiles(USERDATA_ZIP);
	await expect(check).toHaveCSS('opacity', '1');
	await expectFaded(importButton);
	await screenshots(page, dialog, 'fmd2-import-not-checked', testInfo);
});
