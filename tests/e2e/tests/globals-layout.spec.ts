import { test, expect } from '@playwright/test';
import { rawCall } from '../lib/raw';
import { waitForApp, restorePanelType } from '../lib/app';

test.use({ hasTouch: true });

test('globals group markers stay at the right edge on desktop and touch', async ({ page }, testInfo) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		await page.evaluate(async () => {
			const g = (window as any).goofi;
			await g.commands.addGlobal('desk.level', 0.5, 'float', {
				kind: 'knob', min: 0, max: 1, x: 0, y: 0, w: 3, h: 3
			});
			await g.commands.addGlobal('other.value', 1, 'float');
			await g.commands.lockGlobalGroup('desk', { config: true });
			await g.commands.setPanelType(g.query.panels()[0].panelId, 'globals');
		});
		const panel = page.getByTestId('globals-panel');
		const desk = panel.locator('[data-group="desk"]');
		await expect(desk.getByRole('img', { name: 'Control panel' })).toBeVisible();
		for (const width of [1280, 390]) {
			await page.setViewportSize({ width, height: 844 });
			const summary = desk.locator('.ui-disclosure-summary');
			const tags = desk.locator('.grp-tags');
			const rowBox = (await summary.boundingBox())!;
			const tagBox = (await tags.boundingBox())!;
			expect(rowBox.x + rowBox.width - tagBox.x - tagBox.width).toBeLessThan(20);
			expect(tagBox.x).toBeGreaterThan(rowBox.x + rowBox.width / 2);
			const backgrounds = await panel.getByTestId('global-group').evaluateAll((groups) =>
				groups.map((group) => getComputedStyle(group).backgroundColor)
			);
			expect(backgrounds[0]).not.toBe(backgrounds[1]);
			if (width === 390) await summary.tap();
			else await summary.click();
			await expect(desk.getByTestId('global-row')).toBeVisible();
			if (width === 390) await summary.tap();
			else await summary.click();
			await expect(desk.getByTestId('global-row')).toHaveCount(0);
			await page.screenshot({ path: testInfo.outputPath(`globals-${width}.png`) });
		}
	} finally {
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});
