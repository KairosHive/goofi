import { test, expect } from '@playwright/test';
import { appReady, splitRight, closeSplit, restorePanelType } from '../lib/app';
import { addNode, waitForNode } from '../lib/goofi';
import { backendDoc, rawCall } from '../lib/raw';

// The plugin host runs on the test clocks, so an AudioOut here opens no device: naming a cable
// is a document edit, which is all the panel does.
test('a node dropped on a cable row names that cable as its device', async ({ page }) => {
	await page.goto('/');
	await appReady(page);
	const status = (await rawCall(page, 'plugin virtual-cables list')).result;
	test.skip(!!status.unsupported, `no PipeWire here: ${status.unsupported}`);
	const cable = `e2e ${Date.now()}`;
	let split = false;
	try {
		const out = await addNode(page, 'audio:AudioOut', [30, 60]);
		await waitForNode(page, out);
		await splitRight(page);
		split = true;
		await page.evaluate(() => {
			const g = (window as any).goofi;
			const panels = g.query.panels();
			g.commands.setPanelType(panels[panels.length - 1].panelId, 'plugin:virtual-cables:cables');
		});
		const panel = page.getByTestId('cables-panel');
		await expect(panel).toBeVisible();
		await expect(panel.getByTestId('cables-status')).toContainText('0 cables');
		await panel.getByRole('textbox', { name: 'Cable name' }).fill(cable);
		await panel.getByRole('combobox', { name: 'Channels' }).selectOption('4');
		await panel.getByTestId('cables-create').click();
		const row = panel.getByTestId('cable-row');
		await expect(row).toHaveCount(1);
		await expect(row).toContainText(cable);
		await expect(row).toContainText('4ch');

		const header = page.locator(`.svelte-flow__node[data-id="${out}"] .header`);
		const from = (await header.boundingBox())!;
		const to = (await row.boundingBox())!;
		await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
		await page.mouse.down();
		await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 20 });
		await expect(row).toHaveClass(/target/);
		await page.mouse.up();
		await expect.poll(async () => (await backendDoc(page)).nodes[out].params.audio.device.value)
			.toBe(`PipeWire: ${cable}`);
		// The drop landed on the row, so the node never moved on the canvas.
		expect((await header.boundingBox())!.x).toBeCloseTo(from.x, 0);

		await panel.getByRole('button', { name: `Remove ${cable}` }).click();
		await expect(row).toHaveCount(0);
		await expect(panel.getByTestId('cables-status')).toContainText('0 cables');
	} finally {
		await rawCall(page, 'plugin virtual-cables remove', { name: cable });
		if (split) await closeSplit(page);
		await restorePanelType(page);
	}
});
