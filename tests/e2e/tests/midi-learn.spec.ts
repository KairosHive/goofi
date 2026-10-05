import { test, expect, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import path from 'node:path';
import { waitForApp } from '../lib/app';
import { rawCall, backendDoc } from '../lib/raw';
import { addNode, frameSummary, selectNode, nodeParams } from '../lib/goofi';
import { REPO_ROOT, WAIT } from '../playwright.config';

const KEYS = path.join(REPO_ROOT, 'target', 'debug', `midikeys${process.platform === 'win32' ? '.exe' : ''}`);

/** A controller plugged in for the test: a virtual port named `name`, played one message a line.
 * The port leaves with the process, which is how a device is pulled. */
class Keys {
	private child: ChildProcess;
	constructor(name: string) {
		this.child = spawn(KEYS, [name], { stdio: ['pipe', 'pipe', 'inherit'] });
	}
	/** Whether the host listed the port; a machine with no sequencer has none to make. */
	ready(): Promise<boolean> {
		return new Promise((resolve) => {
			this.child.stdout!.once('data', () => resolve(true));
			this.child.once('exit', () => resolve(false));
		});
	}
	play(bytes: number[]): void {
		this.child.stdin!.write(bytes.map((b) => b.toString(16)).join(' ') + '\n');
	}
	unplug(): void {
		this.child.stdin!.end();
		this.child.kill();
	}
}

/** The first frame of a bus entry the tab holds: what learn measures a change against. */
async function baseline(page: Page, name: string): Promise<void> {
	await expect.poll(async () => {
		const frame = await frameSummary(page, 'variables', name);
		return frame !== null && frame.reducedLength === undefined;
	}, { timeout: WAIT }).toBe(true);
}

/** The device groups the backend holds open now. */
async function openGroups(page: Page): Promise<string[]> {
	const groups = (await backendDoc(page)).variable_groups as Record<string, { midi?: unknown }>;
	return Object.keys(groups).filter((g) => groups[g].midi);
}

test('MIDI learn opens every device, maps the first moved element, and keeps only the devices read', async ({ page }) => {
	const keys = new Keys('Test Keys');
	const pads = new Keys('Pads');
	await page.goto('/');
	await waitForApp(page);
	try {
		test.skip(!(await keys.ready()), 'no sequencer to make a virtual port on');
		const consumer = await addNode(page, 'LFO');
		await selectNode(page, consumer);
		await page.getByTestId('param-search').fill('max_frequency');
		const field = page.getByTestId('param-field-max_frequency');
		await field.getByRole('button', { name: 'max_frequency', exact: true }).click();
		const learn = field.getByTestId('param-learn');
		// A learn opens the port the test plugged in, as a group named after it, locked whole.
		await learn.click();
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		await expect(learn.locator('.spinner')).toBeVisible();
		await expect(learn).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
		await expect.poll(() => openGroups(page), { timeout: WAIT }).toContain('midikeys_test_keys');
		await baseline(page, 'midikeys_test_keys.notes');
		keys.play([0x90, 60, 127]);
		await expect(learn).toHaveAttribute('aria-pressed', 'false');
		await expect.poll(async () => (await nodeParams(page, consumer)).common.max_frequency.expression).toBe('variables.midikeys_test_keys.notes[60]');
		await expect(field.getByTestId('param-number')).toHaveValue('1');
		await expect(learn.locator('svg')).toBeVisible();
		// In the variables panel the device's group is locked whole: tagged, no rename, no add.
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'variables');
		});
		const grp = page.locator('[data-testid="variable-group"][data-group="midikeys_test_keys"]');
		await expect(grp).toHaveAttribute('data-lock-config', 'true');
		await expect(grp).toHaveAttribute('data-lock-value', 'true');
		await expect(grp.getByRole('img', { name: 'MIDI device' })).toBeVisible();
		await expect(grp.getByTestId('variable-group-edit')).toHaveCount(0);
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'node-editor');
		});

		// A second device: the next learn listens to both, and the one no longer read is let go.
		expect(await pads.ready()).toBe(true);
		await selectNode(page, consumer);
		await page.getByTestId('param-search').fill('max_frequency');
		if (!(await learn.isVisible())) await field.getByRole('button', { name: 'max_frequency', exact: true }).click();
		await learn.click();
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		await expect.poll(() => openGroups(page), { timeout: WAIT }).toContain('midikeys_pads');
		await baseline(page, 'midikeys_pads.cc');
		pads.play([0xb0, 74, 127]);
		await expect.poll(async () => (await nodeParams(page, consumer)).common.max_frequency.expression).toBe('variables.midikeys_pads.cc[74]');
		await expect(field.getByTestId('param-number')).toHaveValue('1');
		await expect.poll(() => openGroups(page), { timeout: WAIT }).toEqual(['midikeys_pads']);

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
		await expect(widget.locator('.spinner')).toBeVisible();
		await expect.poll(() => openGroups(page), { timeout: WAIT }).toContain('midikeys_test_keys');
		await baseline(page, 'midikeys_test_keys.cc');
		keys.play([0xb0, 74, 127]);
		await expect(widget).toHaveAttribute('aria-pressed', 'false');
		await expect.poll(async () => (await backendDoc(page)).variables?.['desk.level']?.source)
			.toEqual({ reference: 'variables.midikeys_test_keys.cc', index: 74 });
		// A click on a listening learn cancels it.
		await widget.click();
		await expect(widget).toHaveAttribute('aria-pressed', 'true');
		await widget.click();
		await expect(widget).toHaveAttribute('aria-pressed', 'false');
		// A note on channel 2 is the note's slot a channel over.
		await widget.click();
		await baseline(page, 'midikeys_test_keys.notes');
		keys.play([0x91, 61, 100]);
		await expect.poll(async () => (await backendDoc(page)).variables?.['desk.level']?.source)
			.toEqual({ reference: 'variables.midikeys_test_keys.notes', index: 128 + 61 });
		await rawCall(page, 'node param edit', { node: consumer, param: 'common/max_frequency', expression: 'variables.desk.level' });
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'node-editor');
		});
		await selectNode(page, consumer);
		await page.getByTestId('param-search').fill('max_frequency');
		await expect.poll(async () => (await nodeParams(page, consumer)).common.max_frequency.value).toBeCloseTo(100 / 127, 5);
		// Nothing reads the pads now: they are let go. The keys, pulled, take their group with them,
		// and a learn listening to nothing but gone devices stops.
		await expect.poll(() => openGroups(page), { timeout: WAIT }).toEqual(['midikeys_test_keys']);
		if (!(await learn.isVisible())) await field.getByRole('button', { name: 'max_frequency', exact: true }).click();
		await learn.click();
		await expect(learn).toHaveAttribute('aria-pressed', 'true');
		keys.unplug();
		pads.unplug();
		await expect(learn).toHaveAttribute('aria-pressed', 'false', { timeout: WAIT });
		await expect.poll(() => openGroups(page), { timeout: WAIT }).toEqual([]);
	} finally {
		keys.unplug();
		pads.unplug();
		await rawCall(page, 'session new');
	}
});
