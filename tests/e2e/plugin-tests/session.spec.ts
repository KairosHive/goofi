import { test, expect } from '@playwright/test';
import { appReady, restorePanelType } from '../lib/app';
import { rawCall } from '../lib/raw';

// This session runs on the test-clock host in playwright.plugins.config.ts.
test('a folder plugin registers a panel and updates its header entry', async ({ page }) => {
	await page.goto('/');
	await appReady(page);
	const listing = await rawCall(page, 'plugin list');
	expect(listing.result.plugins[0].error).toBeNull();
	await page.evaluate(() => {
		const app = (window as any).goofi;
		app.commands.setPanelType(app.query.panels()[0].panelId, 'plugin:example:corpus');
	});
	await expect(page.getByRole('button', { name: 'Select subject', exact: true })).toBeVisible();
	await page.getByRole('textbox', { name: 'Subject', exact: true }).fill('Alice');
	await page.getByRole('button', { name: 'Select subject', exact: true }).click();
	await expect(page.getByRole('status')).toHaveText('Selected Alice');
	expect((await rawCall(page, 'plugin example status')).result.subject).toBe('Alice');
	const header = page.getByTestId('plugin:example:subject');
	if (await header.isVisible()) {
		await expect(header).toHaveText('Subject: Alice');
		await header.getByRole('button').click();
	} else {
		await page.getByTestId('topbar-overflow').click();
		await page.getByRole('menuitem', { name: 'Subject: Alice', exact: true }).click();
	}
	await expect.poll(async () => (await rawCall(page, 'plugin example status')).result.subject).toBe('Header');
	await page.reload();
	await appReady(page);
	await expect(page.getByRole('button', { name: 'Select subject', exact: true })).toBeVisible();
	await page.getByRole('button', { name: 'Remove header entry' }).click();
	await expect(header).toHaveCount(0);
	expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
	await restorePanelType(page, 'node-editor');
});
