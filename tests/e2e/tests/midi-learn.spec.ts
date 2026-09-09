import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { waitForApp } from '../lib/app';
import { rawCall, backendDoc } from '../lib/raw';
import { addNode, selectNode, updateParam, nodeParams } from '../lib/goofi';

test.use({ actionTimeout: 10_000 });

test('MIDI learn selects a node and maps the first changed channel to a parameter or widget', async ({ page }) => {
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
		await expect(page.getByTestId('toast')).toContainText('Add a MIDI node');
		const status = (await rawCall(page, 'session status')).result;
		const source = path.join(status.workspace, 'nodes_signal', 'learn_midi.py');
		fs.mkdirSync(path.dirname(source), { recursive: true });
		fs.writeFileSync(source, `import goofi
import numpy as np
class LearnMidi(goofi.Node):
    TAGS = ["midi"]
    PRODUCER = True
    OUTPUTS = {"cc": goofi.DataType.ARRAY, "notes": goofi.DataType.ARRAY}
    PARAMS = {"test": {"cc": goofi.FloatParam(0.0, 0.0, 1.0), "note": goofi.FloatParam(0.0, 0.0, 1.0)}}
    def process(self):
        cc = np.zeros(128, dtype=np.float32)
        notes = np.zeros(128, dtype=np.float32)
        cc[74] = self.params.test.cc
        notes[60] = self.params.test.note
        return {"cc": cc, "notes": notes}
`);
		await rawCall(page, 'library refresh');
		const midi = await addNode(page, 'LearnMidi', [350, 0]);
		await rawCall(page, 'node edit', { node: midi, name: 'keys' });
		await selectNode(page, consumer);
		await page.getByTestId('param-search').fill('max_frequency');
		if (!(await learn.isVisible())) await field.getByRole('button', { name: 'max_frequency', exact: true }).click();
		await learn.click();
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		await expect(learn.locator('.spinner')).toBeVisible();
		await expect(learn).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
		// Give the real stream its initial frame before changing a channel.
		await page.waitForTimeout(500);
		await updateParam(page, midi, 'test', 'note', 0.75);
		await expect(learn).toHaveAttribute('aria-pressed', 'false');
		await expect.poll(async () => (await nodeParams(page, consumer)).common.max_frequency.reference).toBe('keys.notes[60]');
		await expect(field.getByTestId('param-number')).toHaveValue('0.75');
		await expect(learn.locator('svg')).toBeVisible();

		const slider = field.getByTestId('param-slider');
		const track = slider.locator('input');
		const bounds = await slider.locator('.ui-slider-bound').allTextContents();
		const descriptor = (await nodeParams(page, consumer)).common.max_frequency;
		const step = await track.getAttribute('step');
		for (const value of [descriptor.vmin - 1, descriptor.vmax + 1, 0.75]) {
			await updateParam(page, midi, 'test', 'note', value);
			await expect.poll(async () => Number(await field.getByTestId('param-number').inputValue())).toBe(value);
			await expect(slider.locator('.ui-slider-bound')).toHaveText(bounds);
			await expect(track).toHaveAttribute('min', String(descriptor.vmin));
			await expect(track).toHaveAttribute('max', String(descriptor.vmax));
			await expect(track).toHaveAttribute('step', step!);
		}


		const second = await addNode(page, 'LearnMidi', [350, 300]);
		await rawCall(page, 'node edit', { node: second, name: 'pads' });
		await selectNode(page, consumer);
		await page.getByTestId('param-search').fill('max_frequency');
		if (!(await learn.isVisible())) await field.getByRole('button', { name: 'max_frequency', exact: true }).click();
		await learn.click();
		await expect(page.getByRole('menuitem', { name: 'keys', exact: true })).toBeVisible();
		await page.getByRole('menuitem', { name: 'pads', exact: true }).click();
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		await page.waitForTimeout(500);
		await updateParam(page, midi, 'test', 'note', 0.875);
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		await updateParam(page, second, 'test', 'cc', 0.5);
		await expect.poll(async () => (await nodeParams(page, consumer)).common.max_frequency.reference).toBe('pads.cc[74]');
		await expect(field.getByTestId('param-number')).toHaveValue('0.5');

		await page.evaluate(async () => {
			const g = (window as any).goofi;
			await g.commands.addGlobal('desk.level', 0.25, 'float', { kind: 'knob', x: 0, y: 0, w: 3, h: 3 });
			await g.commands.addGlobal('desk.text', '', 'string', { kind: 'text', x: 3, y: 0, w: 3, h: 3 });
			const panel = g.query.panels()[0];
			g.commands.setPanelType(panel.panelId, 'control');
			g.commands.setPanelState(panel.panelId, { group: 'desk' });
		});
		await expect(page.getByTestId('control-panel')).toHaveAttribute('data-edit', 'true');
		await expect(page.getByTestId('control-desk-text').getByTestId('control-learn')).toHaveCount(0);
		const widget = page.getByTestId('control-desk-level').getByTestId('control-learn');
		await widget.click();
		await page.getByRole('menuitem', { name: 'keys', exact: true }).click();
		await expect(widget.locator('.spinner')).toBeVisible();
		await page.waitForTimeout(500);
		await updateParam(page, midi, 'test', 'cc', 0.625);
		await expect(widget).toHaveAttribute('aria-pressed', 'false');
		await expect.poll(async () => {
			const doc = await backendDoc(page);
			return doc.globals?.['desk.level']?.source;
		}).toEqual({ reference: 'keys.cc', index: 74 });
		// Relearn always selects a MIDI node, even when a source already exists.
		await widget.click();
		await page.getByRole('menuitem', { name: 'pads', exact: true }).click();
		await expect(widget).toHaveAttribute('aria-pressed', 'true');
		await widget.click();
		await expect(widget).toHaveAttribute('aria-pressed', 'false');
		await rawCall(page, 'node remove', { node: second });
		await widget.click();
		await expect(widget).toHaveAttribute('aria-pressed', 'true');
		await expect(page.getByRole('menu')).toHaveCount(0);
		await page.waitForTimeout(500);
		await updateParam(page, midi, 'test', 'note', 0.375);
		await expect.poll(async () => (await backendDoc(page)).globals?.['desk.level']?.source)
			.toEqual({ reference: 'keys.notes', index: 60 });
		await rawCall(page, 'node param edit', { node: consumer, param: 'common/max_frequency', expression: 'globals.desk.level' });
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
		await rawCall(page, 'node remove', { node: midi });
		await expect(learn).toHaveAttribute('aria-pressed', 'false');
		await learn.click();
		await expect(page.getByTestId('toast')).toContainText('Add a MIDI node');
	} finally {
		await rawCall(page, 'session new');
	}
});
