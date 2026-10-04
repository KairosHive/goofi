import { test, expect, type Page } from '@playwright/test';
import { waitForApp } from '../lib/app';
import { rawCall, backendDoc } from '../lib/raw';
import { addNode, frameSummary, selectNode, nodeParams } from '../lib/goofi';

/** The first frame of a bus entry the tab holds: what learn measures a change against. */
async function baseline(page: Page, name: string): Promise<void> {
	await expect.poll(async () => {
		const frame = await frameSummary(page, 'variables', name);
		return frame !== null && frame.reducedLength === undefined;
	}).toBe(true);
}

/** What a device does: one controller or note of a grabbed group moves, the rest hold. The
 * fixture feeds the store, never a port — a device's group takes a value write as any group does. */
const held = new Map<string, number[]>();
async function feed(page: Page, name: string, index: number, value: number): Promise<void> {
	const frame = held.get(name) ?? Array.from({ length: 128 }, () => 0);
	frame[index] = value;
	held.set(name, frame);
	await rawCall(page, 'variable entry edit', { name, value: frame });
}

test('MIDI learn listens to the bus and maps the first moved element to a parameter or widget', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		const consumer = await addNode(page, 'LFO');
		await selectNode(page, consumer);
		await page.getByTestId('param-search').fill('max_frequency');
		const field = page.getByTestId('param-field-max_frequency');
		await field.getByRole('button', { name: 'max_frequency', exact: true }).click();
		const learn = field.getByTestId('param-learn');
		await learn.click();
		await expect(page.getByTestId('toast')).toContainText('Grab a MIDI device');

		// The MIDI panel grabs a port the host does not list: the grab is the patch's, the port gone.
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'midi');
		});
		await expect(page.getByTestId('midi-panel')).toBeVisible();
		await expect(page.getByTestId('midi-empty').or(page.getByTestId('midi-port').first())).toBeVisible();
		expect((await rawCall(page, 'midi grab', { port: 'Test Keys' })).result.group).toBe('test_keys');
		const row = page.locator('[data-testid="midi-port"][data-group="test_keys"]');
		await expect(row).toHaveAttribute('data-state', 'gone');
		await expect(row.getByTestId('midi-group')).toHaveText('variables.test_keys');
		await expect(row.getByTestId('midi-release')).toBeVisible();
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'node-editor');
		});

		await selectNode(page, consumer);
		await page.getByTestId('param-search').fill('max_frequency');
		if (!(await learn.isVisible())) await field.getByRole('button', { name: 'max_frequency', exact: true }).click();
		await learn.click();
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		await expect(learn.locator('.spinner')).toBeVisible();
		await expect(learn).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
		await baseline(page, 'test_keys.notes');
		await feed(page, 'test_keys.notes', 60, 0.75);
		await expect(learn).toHaveAttribute('aria-pressed', 'false');
		await expect.poll(async () => (await nodeParams(page, consumer)).common.max_frequency.expression).toBe('variables.test_keys.notes[60]');
		await expect(field.getByTestId('param-number')).toHaveValue('0.75');
		await expect(learn.locator('svg')).toBeVisible();

		// A second device: learn asks which, and listens to that one alone.
		expect((await rawCall(page, 'midi grab', { port: 'Pads', group: 'pads' })).result.group).toBe('pads');
		await learn.click();
		await expect(page.getByRole('menuitem', { name: 'test_keys', exact: true })).toBeVisible();
		await page.getByRole('menuitem', { name: 'pads', exact: true }).click();
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		await baseline(page, 'pads.cc');
		await feed(page, 'test_keys.notes', 60, 0.875);
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		await feed(page, 'pads.cc', 74, 0.5);
		await expect.poll(async () => (await nodeParams(page, consumer)).common.max_frequency.expression).toBe('variables.pads.cc[74]');
		await expect(field.getByTestId('param-number')).toHaveValue('0.5');

		await page.evaluate(async () => {
			const g = (window as any).goofi;
			await g.commands.addVariable('desk.level', 0.25, { kind: 'knob', x: 0, y: 0, w: 3, h: 3 });
			await g.commands.addVariable('desk.text', '', { kind: 'text', x: 3, y: 0, w: 3, h: 3 });
			const panel = g.query.panels()[0];
			g.commands.setPanelType(panel.panelId, 'control');
			g.commands.setPanelState(panel.panelId, { group: 'desk' });
		});
		await expect(page.getByTestId('control-panel')).toHaveAttribute('data-edit', 'true');
		await expect(page.getByTestId('control-desk-text').getByTestId('control-learn')).toHaveCount(0);
		const widget = page.getByTestId('control-desk-level').getByTestId('control-learn');
		await widget.click();
		await page.getByRole('menuitem', { name: 'test_keys', exact: true }).click();
		await expect(widget.locator('.spinner')).toBeVisible();
		await baseline(page, 'test_keys.cc');
		await feed(page, 'test_keys.cc', 74, 0.625);
		await expect(widget).toHaveAttribute('aria-pressed', 'false');
		await expect.poll(async () => (await backendDoc(page)).variables?.['desk.level']?.source)
			.toEqual({ reference: 'variables.test_keys.cc', index: 74 });
		// Relearn always asks which device, even when a source already exists; a click cancels.
		await widget.click();
		await page.getByRole('menuitem', { name: 'pads', exact: true }).click();
		await expect(widget).toHaveAttribute('aria-pressed', 'true');
		await widget.click();
		await expect(widget).toHaveAttribute('aria-pressed', 'false');
		// One device left: learn starts on it with no menu.
		await rawCall(page, 'midi release', { group: 'pads' });
		await widget.click();
		await expect(widget).toHaveAttribute('aria-pressed', 'true');
		await expect(page.getByRole('menu')).toHaveCount(0);
		await baseline(page, 'test_keys.notes');
		await feed(page, 'test_keys.notes', 61, 0.375);
		await expect.poll(async () => (await backendDoc(page)).variables?.['desk.level']?.source)
			.toEqual({ reference: 'variables.test_keys.notes', index: 61 });
		await rawCall(page, 'node param edit', { node: consumer, param: 'common/max_frequency', expression: 'variables.desk.level' });
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'node-editor');
		});
		await selectNode(page, consumer);
		await page.getByTestId('param-search').fill('max_frequency');
		await expect(field.getByTestId('param-number')).toHaveValue('0.375');
		if (!(await learn.isVisible())) await field.getByRole('button', { name: 'max_frequency', exact: true }).click();
		await learn.click();
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		// The last device released stops the learn, and the next asks for a device again.
		await rawCall(page, 'midi release', { group: 'test_keys' });
		await expect(learn).toHaveAttribute('aria-pressed', 'false');
		await learn.click();
		await expect(page.getByTestId('toast')).toContainText('Grab a MIDI device');
	} finally {
		await rawCall(page, 'session new');
	}
});
