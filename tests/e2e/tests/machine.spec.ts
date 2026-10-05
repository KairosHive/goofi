// One state machine, built with the pointer the project gives: two cards from a double-click or a
// held finger, a transition dragged between them, a playhead added in the pane, and the dot that
// crosses when the transition is tapped. Runs on the desktop and the phone, and sweeps the scene.

import { test, expect, type Locator, type Page } from '@playwright/test';
import { restorePanelType, waitForApp } from '../lib/app';
import { expectIntact } from '../lib/invariants';
import { rawCall } from '../lib/raw';
import { swipe, touchSession } from '../lib/touch';

/** Whether `inner`'s box sits inside `outer`'s. */
async function within(inner: Locator, outer: Locator): Promise<boolean> {
	const [i, o] = await Promise.all([inner.boundingBox(), outer.boundingBox()]);
	return !!i && !!o && i.x >= o.x && i.y >= o.y && i.x + i.width <= o.x + o.width && i.y + i.height <= o.y + o.height;
}

/** A drag from the centre of `from` to the centre of `to`, with the project's pointer. A finger
 * comes to rest before it lifts, or Chromium reads a fling and eats the tap that follows. */
async function drag(page: Page, hasTouch: boolean, from: Locator, to: Locator): Promise<void> {
	const [a, b] = await Promise.all([from.boundingBox(), to.boundingBox()]);
	const start = { x: a!.x + a!.width / 2, y: a!.y + a!.height / 2 };
	const end = { x: b!.x + b!.width / 2, y: b!.y + b!.height / 2 };
	if (hasTouch) return swipe(page, start, end);
	await page.mouse.move(start.x, start.y);
	await page.mouse.down();
	await page.mouse.move(end.x, end.y, { steps: 8 });
	await page.mouse.up();
}

test('a machine is built by gesture and its playhead crosses on a tap', async ({ page, hasTouch }, testInfo) => {
	await page.goto('/');
	await waitForApp(page);
	const press = (l: Locator): Promise<void> => (hasTouch ? l.tap() : l.click());
	try {
		// A vector widget and a bare array, listed by the variables panel and drawn by a control panel.
		await page.evaluate(async () => {
			const g = (window as any).goofi;
			await g.commands.addVariable('desk.vec', [0.1, 0.2, 0.3], { kind: 'vector', min: 0, max: 1, step: 0.01, x: 0, y: 0, w: 8, h: 2 });
			await g.commands.addVariable('desk.arr', [1, 2, 3, 4]);
			await g.commands.setPanelType(g.query.panels()[0].panelId, 'variables');
		});
		const desk = page.getByTestId('variables-panel').locator('[data-group="desk"]');
		await press(desk.getByTestId('variable-group-toggle'));
		await expect(desk.locator('[data-name="desk.vec"]')).toHaveAttribute('data-control', 'vector');
		await expect(desk.locator('[data-name="desk.arr"]').getByTestId('variable-type').locator('select')).toHaveValue('list');
		await page.evaluate(() => {
			const g = (window as any).goofi;
			const id = g.query.panels()[0].panelId;
			g.commands.setPanelType(id, 'control');
			g.commands.setPanelState(id, { group: 'desk' });
		});
		await expect(page.getByTestId('control-desk-vec').locator('input')).toHaveCount(3);

		// The machine panel starts on its chooser; a new machine is one tap.
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'machine');
		});
		const panel = page.getByTestId('machine-panel');
		await press(panel.getByTestId('machine-new'));
		await expect(panel).toHaveAttribute('data-machine', 'machine0');

		// Two states, each from a double-click or a held finger on the bare canvas.
		const pane = panel.locator('.svelte-flow__pane');
		const box = (await pane.boundingBox())!;
		const addState = async (at: { x: number; y: number }, card: Locator): Promise<void> => {
			if (hasTouch) {
				const touch = await touchSession(page);
				await touch.down(at);
				await expect(card, 'the held finger added a state').toBeVisible();
				await touch.up();
			} else {
				await page.mouse.dblclick(at.x, at.y);
				await expect(card).toBeVisible();
			}
		};
		const card0 = page.getByTestId('state-card-state0');
		const card1 = page.getByTestId('state-card-state1');
		// Staggered, so on a phone the edge's label lands between the cards and above the pane.
		await addState({ x: box.x + box.width * 0.3, y: box.y + box.height * 0.18 }, card0);
		await addState({ x: box.x + box.width * 0.7, y: box.y + box.height * 0.42 }, card1);

		// A transition dragged from one card's port to the other's; it opens its pane, picked.
		await drag(page, hasTouch,
			page.locator('.svelte-flow__handle.source[data-nodeid="state0"]'),
			page.locator('.svelte-flow__handle.target[data-nodeid="state1"]'));
		const label = page.getByTestId('transition-t1');
		await expect(label).toHaveText('tap');
		await expect(page.getByTestId('machine-transition')).toBeVisible();

		// A playhead is born in the first state, and its dot sits on that card.
		await press(page.getByTestId('machine-add-playhead'));
		const dot = page.getByTestId('playhead-dot');
		await expect(dot).toHaveAttribute('data-state', 'state0');
		await expect.poll(() => within(dot, card0)).toBe(true);
		await expect(page.getByTestId('playhead-state')).toHaveText('state0');

		// The tap fires it, and the dot comes to rest on the target card.
		await press(label);
		await expect(dot).toHaveAttribute('data-state', 'state1');
		await expect(dot).toHaveAttribute('data-flying', 'false');
		await expect.poll(() => within(dot, card1)).toBe(true);
		await expect(page.getByTestId('playhead-state')).toHaveText('state1');

		// The playhead's group is the machine's: listed locked whole, like a device's.
		const groups = (await rawCall(page, 'session state')).result.variable_groups;
		expect(groups.playhead0.machine).toBe('machine0');

		await expectIntact(page, 'the machine panel');
		await page.screenshot({ path: testInfo.outputPath('machine.png') });
	} finally {
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});
