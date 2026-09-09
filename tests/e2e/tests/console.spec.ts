import { test, expect } from '@playwright/test';
import { rawCall } from '../lib/raw';
import { appReady, waitForApp, restorePanelType } from '../lib/app';

test.use({ hasTouch: true });

test('console groups repeats, filters history and runs the shared op vocabulary', async ({ page }, testInfo) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		await page.evaluate(async () => {
			const g = (window as any).goofi;
			await g.commands.setPanelType(g.query.panels()[0].panelId, 'console');
		});
		await rawCall(page, 'log clear');
		for (const text of ['first message', 'middle message', 'first message']) {
			expect((await rawCall(page, 'log write', { text, component: 'fixture', level: 'warning' })).error).toBeUndefined();
		}
		const panel = page.getByTestId('console-panel');
		const filter = panel.getByRole('textbox', { name: 'Filter messages' });
		await filter.fill('fixture');
		const rows = panel.getByTestId('console-entry');
		await expect(rows).toHaveCount(2);
		await expect(rows.nth(0)).toContainText('middle message');
		await expect(rows.nth(1)).toContainText('first message');
		await expect(rows.nth(1).getByTestId('console-count')).toHaveText('×2');
		await panel.getByRole('button', { name: 'warning', exact: true }).click();
		await expect(rows).toHaveCount(0);
		await panel.getByRole('button', { name: 'warning', exact: true }).click();
		await expect(rows).toHaveCount(2);
		await filter.fill('middle');
		await expect(rows).toHaveCount(1);
		await page.reload();
		await appReady(page);
		await filter.fill('fixture');
		await expect(rows).toHaveCount(2);
		await expect(rows.nth(1).getByTestId('console-count')).toHaveText('×2');
		await filter.fill('');
		const input = panel.getByRole('textbox', { name: 'Console command' });
		await input.fill('global entry add console.value --value 7 --type int');
		await input.press('Enter');
		await expect.poll(async () => (await rawCall(page, 'global list')).result.globals?.some((e: any) => e.name === 'console.value')).toBe(true);
		await input.press('ArrowUp');
		await expect(input).toHaveValue('global entry add console.value --value 7 --type int');
		await input.fill('invalid-command');
		await expect(panel.getByRole('button', { name: 'Run', exact: true })).toBeEnabled();
		await input.press('Enter');
		await expect(rows.filter({ hasText: 'unknown op' })).toHaveCount(1);
		await expect(panel.getByRole('alert')).toHaveCount(0);
		await input.fill('log cl');
		await input.press('Tab');
		await expect(input).toHaveValue('log clear ');
		for (const width of [1280, 390]) {
			await page.setViewportSize({ width, height: 844 });
			await expect(input).toBeVisible();
			expect(await panel.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
			await page.screenshot({ path: testInfo.outputPath(`console-${width}.png`) });
		}
		await panel.getByRole('button', { name: 'Clear console', exact: true }).click();
		await expect(rows).toHaveCount(0);
	} finally {
		await rawCall(page, 'global entry remove', { name: 'console.value' });
		await restorePanelType(page);
	}
});
