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
		await input.fill('log li');
		await input.press('Tab');
		await expect(input).toHaveValue('log list ');
		const messages = [
			'selection-fixture first\nline two\nline three\nline four\nline five\nline six',
			'selection-fixture second',
			'selection-fixture third'
		];
		for (const index of [0, 1, 1, 2]) {
			await rawCall(page, 'log write', { text: messages[index], component: index === 1 ? 'command' : 'goofi' });
		}
		await filter.fill('selection-fixture');
		await expect(rows).toHaveCount(3);
		await expect(rows.locator('.node')).toHaveText(['g', 'c', 'g']);
		await expect(rows.nth(0).locator('.node')).toHaveAttribute('title', 'goofi');
		await expect(rows.nth(1).getByTestId('console-count')).toHaveText('×2');
		const firstText = rows.nth(0).locator('.txt');
		await expect(firstText).toHaveText(messages[0]);
		expect(await firstText.evaluate((el) => el.clientHeight >= el.scrollHeight)).toBe(true);
		expect(await rows.evaluateAll((els) => els.every((el) => getComputedStyle(el).borderBottomWidth === '0px'))).toBe(true);
		await expect(panel.locator('.caret')).toHaveCount(0);
		for (const row of await rows.all()) {
			const time = row.locator('time');
			const expected = await time.evaluate((el) => new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' }).format(new Date(el.getAttribute('title')!)));
			await expect(time).toHaveText(expected);
		}
		await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
		const points = await rows.locator('.txt').evaluateAll((els) => {
			const point = (el: Element, offset: number) => {
				const range = document.createRange();
				range.setStart(el.firstChild!, offset);
				range.setEnd(el.firstChild!, offset + 1);
				const box = range.getBoundingClientRect();
				return { x: box.x, y: box.y + box.height / 2, right: box.right };
			};
			return { start: point(els[0], 0), end: point(els[2], els[2].textContent!.length - 1) };
		});
		await page.mouse.move(points.start.x, points.start.y);
		await page.mouse.down();
		await page.mouse.move(points.end.right, points.end.y, { steps: 20 });
		await page.mouse.up();
		await page.keyboard.press('ControlOrMeta+c');
		await expect.poll(async () => (await page.evaluate(() => navigator.clipboard.readText())).split('\n').filter(Boolean))
			.toEqual(messages.flatMap((message) => message.split('\n')));
		await page.evaluate(() => window.getSelection()?.removeAllRanges());
		for (const width of [1280, 390]) {
			await page.setViewportSize({ width, height: 844 });
			await expect(input).toBeVisible();
			expect(await panel.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
			await page.screenshot({ path: testInfo.outputPath(`console-${width}.png`) });
		}
		await expect(panel.getByRole('button', { name: 'Clear console', exact: true })).toHaveCount(0);
		expect((await rawCall(page, 'log clear')).error).toContain('unknown op');
	} finally {
		await rawCall(page, 'global entry remove', { name: 'console.value' });
		await restorePanelType(page);
	}
});
