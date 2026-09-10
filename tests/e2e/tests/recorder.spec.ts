import { test, expect, type Locator } from '@playwright/test';
import { appReady, waitForApp, splitRight, closeSplit, restorePanelType } from '../lib/app';
import { addNode, waitForNode } from '../lib/goofi';
import { armSocketControl, dropSocket, restoreSocket, backendDoc, rawCall } from '../lib/raw';

for (const narrow of [false, true]) {
	test.describe(narrow ? 'narrow touch screen' : 'desktop', () => {
		test.use({ hasTouch: narrow });
		test(`recorder selects outputs and stores row quality${narrow ? ' on a narrow screen' : ''}`, async ({ page }, testInfo) => {
			if (narrow) await page.setViewportSize({ width: 390, height: 844 });
			await page.goto('/');
			await waitForApp(page);
			const press = (target: Locator) => narrow ? target.tap() : target.click();
			let split = false;
			try {
				const multi = await addNode(page, 'signal:Quantize', [30, 60]);
				const video = await addNode(page, 'graphics:Constant', [300, 60]);
				await waitForNode(page, video);
				const doc = await backendDoc(page);
				if (!narrow) {
					await splitRight(page);
					split = true;
				}
				await page.evaluate(() => {
					const g = (window as any).goofi;
					const panels = g.query.panels();
					g.commands.setPanelType(panels[panels.length - 1].panelId, 'recorder');
				});
				const panel = page.getByTestId('recorder-panel');
				await expect(panel).toBeVisible();
				const rows = panel.getByTestId('recorder-stream');
				const openMulti = async () => {
					await press(panel.getByTestId('recorder-add-node'));
					await press(page.getByRole('menuitem', { name: doc.nodes[multi].name, exact: true }));
					await expect(page.getByRole('menuitemcheckbox', { name: 'out', exact: true })).toHaveAttribute('aria-checked', 'true');
					await expect(page.getByRole('menuitemcheckbox', { name: 'index', exact: true })).toHaveAttribute('aria-checked', 'true');
				};
				await openMulti();
				await press(page.getByRole('menuitemcheckbox', { name: 'index', exact: true }));
				await expect(page.getByRole('menuitemcheckbox', { name: 'index', exact: true })).toHaveAttribute('aria-checked', 'false');
				await expect(rows).toHaveCount(0);
				await press(panel.getByTestId('recorder-elapsed'));
				await expect(rows).toHaveCount(1);
				await expect(rows.first()).toHaveAttribute('data-output', `${multi}/out`);
				await expect(rows.first()).toContainText('Native');
				await page.evaluate(() => (window as any).goofi.commands.undo());
				await expect(rows).toHaveCount(0);

				if (!narrow) {
					const header = page.locator(`.svelte-flow__node[data-id="${multi}"] .header`);
					const from = (await header.boundingBox())!;
					const to = (await panel.boundingBox())!;
					await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
					await page.mouse.down();
					await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 20 });
					await page.mouse.up();
					await expect(page.getByRole('menuitemcheckbox', { name: 'index', exact: true })).toHaveAttribute('aria-checked', 'true');
				} else {
					await openMulti();
				}
				await press(page.getByRole('menuitem', { name: 'Deselect all', exact: true }));
				await expect(page.getByRole('menuitemcheckbox', { name: 'out', exact: true })).toHaveAttribute('aria-checked', 'false');
				await press(page.getByRole('menuitem', { name: 'Select all', exact: true }));
				await expect(page.getByRole('menuitemcheckbox', { name: 'out', exact: true })).toHaveAttribute('aria-checked', 'true');
				await press(panel.getByTestId('recorder-elapsed'));
				await expect(rows).toHaveCount(2);
				await page.evaluate(() => (window as any).goofi.commands.undo());
				await expect(rows).toHaveCount(0);
				await openMulti();
				await press(page.getByRole('menuitem', { name: 'Deselect all', exact: true }));
				await press(panel.getByTestId('recorder-elapsed'));
				await expect(rows).toHaveCount(0);

				await press(panel.getByTestId('recorder-add-node'));
				await press(page.getByRole('menuitem', { name: doc.nodes[video].name, exact: true }));
				await expect(rows).toHaveCount(1);
				const quality = rows.getByLabel('Quality', { exact: true });
				await expect(quality).toHaveValue('high');
				await quality.selectOption('very_high');
				await expect(quality).toHaveValue('very_high');
				expect((await backendDoc(page)).nodes[video].record).toEqual([{ slot: 'out', quality: 'very_high' }]);
				await page.evaluate(() => (window as any).goofi.commands.undo());
				await expect(quality).toHaveValue('high');
				const dimensions = await panel.evaluate((el) => ({ width: el.clientWidth, content: el.scrollWidth }));
				expect(dimensions.content).toBeLessThanOrEqual(dimensions.width + 1);
				await page.screenshot({ path: testInfo.outputPath('recorder.png') });
			} finally {
				if (split) await closeSplit(page);
				await restorePanelType(page);
				await rawCall(page, 'session new');
			}
		});
	});
}

test('recorder refreshes after patch load and reconnect', async ({ page }, testInfo) => {
	await armSocketControl(page);
	await page.goto('/');
	await waitForApp(page);
	const node = await addNode(page, 'signal:Quantize', [30, 60]);
	await rawCall(page, 'record arm', { output: `${node}/out` });
	await page.evaluate(() => {
		const g = (window as any).goofi;
		g.commands.setPanelType(g.query.panels()[0].panelId, 'recorder');
	});
	const toggle = page.getByTestId('recorder-toggle');
	const root = testInfo.outputPath('recordings');
	const path = testInfo.outputPath('recording.gfi');
	try {
		expect((await rawCall(page, 'record start', { root })).error).toBeUndefined();
		await expect(toggle).toHaveText('Stop');
		expect((await rawCall(page, 'session save', { path })).error).toBeUndefined();
		expect((await rawCall(page, 'session load', { path })).error).toBeUndefined();
		await expect(toggle).toHaveText('Start');
		expect((await rawCall(page, 'record status')).result.running).toBe(false);
		expect((await rawCall(page, 'record start', { root })).error).toBeUndefined();
		await expect(toggle).toHaveText('Stop');
		await dropSocket(page);
		// A separate client stops the recording while this tab cannot receive events.
		const peer = await page.context().newPage();
		await peer.goto('/');
		expect((await rawCall(peer, 'record stop')).error).toBeUndefined();
		await peer.close();
		await restoreSocket(page);
		await expect(toggle).toHaveText('Start');
		await page.reload();
		await appReady(page);
		await expect(toggle).toHaveText('Start');
	} finally {
		await restoreSocket(page);
		await rawCall(page, 'session new');
		await restorePanelType(page);
	}
});
