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
			const summary = desk.getByTestId('global-group-toggle');
			const tags = desk.locator('.grp-tags');
			const rowBox = (await desk.locator('.grp-head').boundingBox())!;
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
		await page.setViewportSize({ width: 390, height: 844 });
		await panel.getByTestId('global-add-group-btn').tap();
		let group = panel.locator('[data-group="group0"]');
		let input = group.getByTestId('global-group-name');
		await expect(input).toBeFocused();
		await expect(input).toHaveValue('group0');
		expect(await input.evaluate((el: HTMLInputElement) => el.selectionEnd! - el.selectionStart!)).toBe(6);
		await input.fill('fresh');
		await input.press('Enter');
		group = panel.locator('[data-group="fresh"]');
		await expect(group).toBeVisible();
		await expect(group.getByTestId('global-add-in')).toHaveCount(0);
		await group.getByTestId('global-group-toggle').tap();
		const add = group.getByTestId('global-add-in');
		await add.tap();
		let row = group.locator('[data-name="fresh.entry0"]');
		await expect(row.getByTestId('global-name')).toBeFocused();
		await add.tap();
		await expect(group.getByTestId('global-row')).toHaveCount(2);
		row = group.locator('[data-name="fresh.entry1"]');
		await expect(row.getByTestId('global-name')).toBeFocused();
		await row.getByTestId('global-name').fill('message');
		await row.getByTestId('global-name').press('Enter');
		row = group.locator('[data-name="fresh.message"]');
		await row.getByTestId('global-type').locator('select').selectOption('string');
		await row.locator('input.ui-text[data-testid="global-value"]').fill('hello');
		await row.getByTestId('global-value').press('Enter');
		await expect.poll(() => page.evaluate(() => (window as any).goofi.query.globals()
			.find((entry: { name: string }) => entry.name === 'fresh.message'))).toMatchObject({ type: 'string', value: 'hello' });
		await expect(panel.locator('[data-testid^="global-lock"], [data-testid^="global-group-lock"]')).toHaveCount(0);
		await group.getByTestId('global-group-edit').tap();
		input = group.getByTestId('global-group-name');
		await expect(input).toBeFocused();
		await input.fill('renamed');
		await input.press('Enter');
		group = panel.locator('[data-group="renamed"]');
		await expect(group.getByTestId('global-row')).toHaveCount(2);
		await page.screenshot({ path: testInfo.outputPath('globals-editing-phone.png') });
		await panel.getByTestId('global-add-group-btn').tap();
		await expect(panel.locator('[data-group="group0"]').getByTestId('global-group-name')).toBeFocused();
		await panel.locator('[data-group="group0"]').getByTestId('global-group-name').press('Escape');
		await panel.getByTestId('global-add-group-btn').tap();
		await expect(panel.locator('[data-group="group1"]').getByTestId('global-group-name')).toBeFocused();

	} finally {
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});
