import { test, expect, type Locator, type Page } from '@playwright/test';
import { appReady, splitRight, closeSplit, restorePanelType } from '../lib/app';
import { addNode, waitForNode } from '../lib/goofi';
import { backendDoc, rawCall } from '../lib/raw';

/** Drag a node's header onto one of the panel's rows, and wait for the row to take it. */
async function drop(page: Page, uid: string, row: Locator): Promise<void> {
	const header = page.locator(`.svelte-flow__node[data-id="${uid}"] .header`);
	const from = (await header.boundingBox())!;
	const to = (await row.boundingBox())!;
	await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
	await page.mouse.down();
	await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 20 });
	await expect(row).toHaveClass(/target/);
	await page.mouse.up();
	await expect(row.locator('.name')).not.toHaveText('Drop a node here');
}

/** The authored state of the first panel of `type` in the manager's arrangement. */
function panelState(doc: any, type: string): any {
	const walk = (entry: any): any =>
		entry.kind === 'panel'
			? entry.panel_type === type ? entry.state : null
			: (entry.children ?? []).map(walk).find((s: any) => s) ?? null;
	const arrangement = doc.arrangement ?? doc.patch?.arrangement;
	return arrangement.tabs.map((tab: any) => walk(tab.root)).find((s: any) => s) ?? null;
}

test('two nodes dropped on the latency panel are timed tick by tick into a CSV', async ({ page }) => {
	await page.goto('/');
	await appReady(page);
	const made: string[] = [];
	let split = false;
	try {
		const number = await addNode(page, 'signal:PluginNumber', [30, 60]);
		made.push(number);
		await waitForNode(page, number);
		const echo = await addNode(page, 'signal:PluginEcho', [30, 300]);
		made.push(echo);
		await waitForNode(page, echo);
		expect((await rawCall(page, 'link add', { from: `${number}/out`, to: `${echo}/input` })).error).toBeUndefined();
		await splitRight(page);
		split = true;
		await page.evaluate(() => {
			const g = (window as any).goofi;
			const panels = g.query.panels();
			g.commands.setPanelType(panels[panels.length - 1].panelId, 'plugin:latency:latency');
		});
		const panel = page.getByTestId('latency-panel');
		await expect(panel).toBeVisible();
		const toggle = panel.getByTestId('latency-toggle');
		await expect(toggle).toBeDisabled();
		await drop(page, number, panel.getByTestId('latency-source'));
		await drop(page, echo, panel.getByTestId('latency-target'));
		await expect(toggle).toBeEnabled();
		// The two ends are the panel's authored state, so a saved patch keeps them.
		const state = panelState(await backendDoc(page), 'plugin:latency:latency');
		expect([state.source, state.target]).toEqual([number, echo]);

		await toggle.click();
		await expect(toggle).toHaveText('Stop');
		await expect(panel.getByTestId('latency-status')).toContainText('Recording');
		await expect.poll(async () => Number(await panel.locator('[data-stat=ticks]').innerText())).toBeGreaterThan(4);
		expect((await rawCall(page, 'record status')).result.running).toBe(true);
		await toggle.click();
		await expect(toggle).toHaveText('Start');
		await expect(panel.getByTestId('latency-status')).toContainText('Done');
		await expect(panel.getByTestId('latency-csv')).toContainText('latency.csv');
		await expect(panel.locator('[data-stat=mean]')).toContainText('ms');
		expect((await rawCall(page, 'record status')).result.running).toBe(false);
	} finally {
		await rawCall(page, 'plugin latency stop');
		for (const uid of made) await rawCall(page, 'node remove', { node: uid });
		if (split) await closeSplit(page);
		await restorePanelType(page);
	}
});
