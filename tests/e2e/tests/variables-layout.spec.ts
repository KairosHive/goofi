import { test, expect } from '@playwright/test';
import { rawCall } from '../lib/raw';
import { waitForApp, restorePanelType } from '../lib/app';

test.use({ hasTouch: true });

test('variables group markers stay at the right edge on desktop and touch', async ({ page }, testInfo) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		await page.evaluate(async () => {
			const g = (window as any).goofi;
			await g.commands.addVariable('desk.level', 0.5, 'float', {
				kind: 'knob', min: 0, max: 1, x: 0, y: 0, w: 3, h: 3
			});
			await g.commands.addVariable('other.value', 1, 'float');
			await g.commands.setPanelType(g.query.panels()[0].panelId, 'variables');
		});
		const panel = page.getByTestId('variables-panel');
		const desk = panel.locator('[data-group="desk"]');
		await expect(desk.getByRole('img', { name: 'Control panel' })).toBeVisible();
		await expect(desk).toHaveAttribute('data-lock-config', 'true');
		await expect(desk.getByTestId('variable-group-edit')).toHaveCount(0);
		const markers = await desk.locator('.grp-tags').evaluate((tags) => ({
			last: tags.lastElementChild?.className,
			control: getComputedStyle(tags.querySelector('.grp-control')!).color,
			lock: getComputedStyle(tags.querySelector('.grp-lock')!).color
		}));
		expect(markers.last).toContain('grp-count');
		expect(markers.control).toBe(markers.lock);
		for (const width of [1280, 390]) {
			await page.setViewportSize({ width, height: 844 });
			const summary = desk.getByTestId('variable-group-toggle');
			const tags = desk.locator('.grp-tags');
			const rowBox = (await desk.locator('.grp-head').boundingBox())!;
			const tagBox = (await tags.boundingBox())!;
			expect(rowBox.x + rowBox.width - tagBox.x - tagBox.width).toBeLessThan(20);
			expect(tagBox.x).toBeGreaterThan(rowBox.x + rowBox.width / 2);
			const backgrounds = await panel.locator('.grp-head').evaluateAll((groups) =>
				groups.map((group) => getComputedStyle(group).backgroundColor)
			);
			expect(new Set(backgrounds).size).toBe(1);
			const middle = { x: rowBox.width / 2, y: rowBox.height / 2 };
			if (width === 390) await summary.tap({ position: middle });
			else await summary.click({ position: middle });
			await expect(desk.getByTestId('variable-row')).toBeVisible();
			expect(await desk.locator('.grp-body').evaluate((body) => getComputedStyle(body).backgroundColor)).not.toBe(backgrounds[0]);
			await expect(desk.getByTestId('variable-name')).toHaveCount(0);
			await expect(desk.getByTestId('variable-delete')).toHaveCount(0);
			await expect(desk.getByTestId('variable-add-in')).toHaveCount(0);
			await expect(desk.getByTestId('variable-type').locator('select')).toBeDisabled();
			await desk.getByTestId('variable-value').fill('0.75');
			await desk.getByTestId('variable-value').press('Enter');
			await expect.poll(() => page.evaluate(() => (window as any).goofi.query.variables()
				.find((entry: { name: string }) => entry.name === 'desk.level').value)).toBe(0.75);
			expect(await panel.getByTestId('variable-group').evaluateAll((groups) => groups.every((group, index) =>
				getComputedStyle(group).borderBottomWidth === (index === groups.length - 1 ? '1px' : '0px')
			))).toBe(true);
			if (width === 390) await summary.tap();
			else await summary.click();
			await expect(desk.getByTestId('variable-row')).toHaveCount(0);
			await page.screenshot({ path: testInfo.outputPath(`variables-${width}.png`) });
		}
		await page.setViewportSize({ width: 390, height: 844 });
		await panel.getByTestId('variable-add-group-btn').tap();
		let group = panel.locator('[data-group="group0"]');
		let input = group.getByTestId('variable-group-name');
		await expect(input).toBeFocused();
		await expect(input).toHaveValue('group0');
		expect(await input.evaluate((el: HTMLInputElement) => el.selectionEnd! - el.selectionStart!)).toBe(6);
		await input.fill('fresh');
		await input.press('Enter');
		group = panel.locator('[data-group="fresh"]');
		await expect(group).toBeVisible();
		await expect(group.getByTestId('variable-add-in')).toHaveCount(0);
		await group.getByTestId('variable-group-toggle').tap();
		const add = group.getByTestId('variable-add-in');
		await add.tap();
		let row = group.locator('[data-name="fresh.entry0"]');
		await expect(row.getByTestId('variable-name')).toBeFocused();
		await add.tap();
		await expect(group.getByTestId('variable-row')).toHaveCount(2);
		expect(await group.getByTestId('variable-row').evaluateAll((rows) => rows.map((row) => ({
			top: getComputedStyle(row).borderTopWidth,
			bottom: getComputedStyle(row).borderBottomWidth
		})))).toEqual([{ top: '0px', bottom: '0px' }, { top: '1px', bottom: '0px' }]);
		row = group.locator('[data-name="fresh.entry1"]');
		await expect(row.getByTestId('variable-name')).toBeFocused();
		await row.getByTestId('variable-name').fill('message');
		await row.getByTestId('variable-name').press('Enter');
		row = group.locator('[data-name="fresh.message"]');
		await row.getByTestId('variable-type').locator('select').selectOption('string');
		await row.locator('input.ui-text[data-testid="variable-value"]').fill('hello');
		await row.getByTestId('variable-value').press('Enter');
		await expect.poll(() => page.evaluate(() => (window as any).goofi.query.variables()
			.find((entry: { name: string }) => entry.name === 'fresh.message'))).toMatchObject({ type: 'string', value: 'hello' });
		await expect(panel.locator('[data-testid^="variable-lock"], [data-testid^="variable-group-lock"]')).toHaveCount(0);
		await group.getByTestId('variable-group-edit').tap();
		input = group.getByTestId('variable-group-name');
		await expect(input).toBeFocused();
		await input.fill('renamed');
		await input.press('Enter');
		group = panel.locator('[data-group="renamed"]');
		await expect(group.getByTestId('variable-row')).toHaveCount(2);
		for (const [button, row] of [
			[group.getByTestId('variable-add-in'), group.getByTestId('variable-row').first()],
			[panel.getByTestId('variable-add-group-btn'), group]
		]) {
			const buttonBox = (await button.boundingBox())!;
			const rowBox = (await row.boundingBox())!;
			expect(Math.abs(buttonBox.width - rowBox.width)).toBeLessThan(1);
			expect(Math.abs(buttonBox.x - rowBox.x)).toBeLessThan(1);
			expect(await button.evaluate((element) => getComputedStyle(element).justifyContent)).toBe('center');
		}
		const editedRow = group.locator('[data-name="renamed.message"]');
		const nameBox = (await editedRow.getByTestId('variable-name').boundingBox())!;
		const valueBox = (await editedRow.getByTestId('variable-value').boundingBox())!;
		expect(Math.abs(nameBox.y - valueBox.y)).toBeLessThan(1);
		expect(await panel.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
		await page.screenshot({ path: testInfo.outputPath('variables-editing-phone.png') });
		await panel.getByTestId('variable-add-group-btn').tap();
		await expect(panel.locator('[data-group="group0"]').getByTestId('variable-group-name')).toBeFocused();
		await panel.locator('[data-group="group0"]').getByTestId('variable-group-name').press('Escape');
		await panel.getByTestId('variable-add-group-btn').tap();
		await expect(panel.locator('[data-group="group1"]').getByTestId('variable-group-name')).toBeFocused();

	} finally {
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});
