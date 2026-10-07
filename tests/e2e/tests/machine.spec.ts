// One state machine, built with the pointer the project gives: two boxes from a double-click or a
// held finger, a transition dragged from one box's edge to the other, a playhead added in the
// inspector, and the dot that crosses when the transition is tapped. Runs on the desktop and the
// phone, and sweeps the scene.

import { test, expect, type Locator, type Page } from '@playwright/test';
import { restorePanelType, waitForApp } from '../lib/app';
import { expectIntact } from '../lib/invariants';
import { rawCall } from '../lib/raw';
import { swipe, touchSession } from '../lib/touch';

/** Whether `dot`'s centre sits on `card`'s top edge. */
async function onTopEdge(dot: Locator, card: Locator): Promise<boolean> {
	const [d, c] = await Promise.all([dot.boundingBox(), card.boundingBox()]);
	if (!d || !c) return false;
	const cx = d.x + d.width / 2;
	const cy = d.y + d.height / 2;
	return cx >= c.x && cx <= c.x + c.width && Math.abs(cy - c.y) <= 2;
}

/** A drag from a point on `from`'s right edge to a point well off `to`'s centre, with the project's
 * pointer. A finger comes to rest before it lifts, or Chromium reads a fling and eats the tap that follows. */
async function drag(page: Page, hasTouch: boolean, from: Locator, to: Locator): Promise<void> {
	const [a, b] = await Promise.all([from.boundingBox(), to.boundingBox()]);
	const start = { x: a!.x + a!.width - 3, y: a!.y + a!.height / 2 };
	const end = { x: b!.x + b!.width * 0.25, y: b!.y + b!.height * 0.4 };
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

		// A machine panel on a patch without a machine starts one, and shows it.
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'machine');
		});
		const panel = page.getByTestId('machine-panel');
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
		// In the canvas the inspector leaves bare: its pane takes the right side, or the bottom on a phone.
		await addState({ x: box.x + box.width * 0.2, y: box.y + box.height * 0.12 }, card0);
		await addState({ x: box.x + box.width * 0.5, y: box.y + box.height * 0.3 }, card1);

		// A transition dragged from one box's edge into the other; the inspector shows it, selected.
		await drag(page, hasTouch, card0, card1);
		const label = page.getByTestId('transition-t1');
		await expect(label).toHaveText('manual');
		const inspector = page.getByTestId('machine-inspector');
		await expect(inspector).toHaveAttribute('data-subject', 'transition');
		await expect(page.getByTestId('machine-transition')).toBeVisible();

		// A box selected shows its name and the transition out of it; the bare canvas shows the machine.
		await press(card0);
		await expect(inspector).toHaveAttribute('data-subject', 'state');
		await expect(inspector.getByTestId('state-name')).toHaveText('state0');
		await expect(inspector.getByTestId('transitions-out').getByTestId('transition-row-t1')).toBeVisible();
		await expect(inspector.getByTestId('transitions-in').getByTestId('transition-row-t1')).toHaveCount(0);
		const corner = { x: box.x + box.width * 0.55, y: box.y + box.height * 0.04 };
		if (hasTouch) await page.touchscreen.tap(corner.x, corner.y);
		else await page.mouse.click(corner.x, corner.y);
		await expect(inspector).toHaveAttribute('data-subject', 'machine');

		// A playhead is born in the first state, and its dot sits on that box's top edge.
		await press(page.getByTestId('machine-add-playhead'));
		const dot = page.getByTestId('playhead-dot');
		await expect(dot).toHaveAttribute('data-state', 'state0');
		await expect.poll(() => onTopEdge(dot, card0)).toBe(true);
		await expect(page.getByTestId('playhead-state')).toHaveText('state0');

		// The label is gone while nothing of it is selected; a box of the transition brings it back, and
		// a tap on it selects it. The inspector's fire button moves the playhead, and the dot comes to
		// rest on the target box.
		await expect(label).toHaveCount(0);
		await press(card1);
		await press(label);
		await expect(inspector).toHaveAttribute('data-subject', 'transition');
		await press(inspector.getByTestId('transition-fire'));
		await expect(dot).toHaveAttribute('data-state', 'state1');
		await expect(dot).toHaveAttribute('data-flying', 'false');
		await expect.poll(() => onTopEdge(dot, card1)).toBe(true);
		if (hasTouch) await page.touchscreen.tap(corner.x, corner.y);
		else await page.mouse.click(corner.x, corner.y);
		await expect(page.getByTestId('playhead-state')).toHaveText('state1');

		// The machine's name and seed are the inspector's; the bar's tabs are the switch between machines.
		await expect(inspector.getByTestId('machine-name')).toHaveText('machine0');
		await expect(inspector.getByTestId('machine-seed')).toHaveValue('0');
		const tabs = panel.getByTestId('machine-tabs');
		await expect(tabs.locator('[role=tab][aria-selected="true"]')).toHaveText('machine0');

		// An attribute is drawn as a param: a number's slider, and the kind select offers the faces.
		await press(inspector.getByTestId('machine-add-attribute'));
		const attr = inspector.getByTestId('attribute-attr0');
		await expect(attr).toHaveAttribute('data-face', 'number');
		await expect(attr.getByTestId('param-slider')).toBeVisible();
		await attr.getByTestId('attribute-kind').locator('select').selectOption('toggle');
		await expect(attr).toHaveAttribute('data-face', 'toggle');
		await expect(attr.getByTestId('param-toggle')).toBeVisible();

		await page.screenshot({ path: testInfo.outputPath('machine.png') });

		// A transition back the other way runs beside the first, its label apart from the first's, and
		// either label selects its own. The labels show once a box of theirs is selected.
		await rawCall(page, 'machine transition add', { machine: 'machine0', from: 'state1', to: 'state0', triggers: [{ kind: 'manual' }] });
		const back = page.getByTestId('transition-t2');
		await expect(back).toHaveCount(0);
		await press(card0);
		await expect(back).toBeVisible();
		const [l1, l2] = await Promise.all([label.boundingBox(), back.boundingBox()]);
		expect(l1!.x + l1!.width <= l2!.x || l2!.x + l2!.width <= l1!.x || l1!.y + l1!.height <= l2!.y || l2!.y + l2!.height <= l1!.y, `the two labels do not overlap: ${JSON.stringify(l1)} ${JSON.stringify(l2)}`).toBe(true);
		await press(back);
		await expect(page.locator('.svelte-flow__edge.selected')).toHaveAttribute('data-id', /t2/);
		await page.keyboard.press('Delete');
		await expect(back).toHaveCount(0);

		// The first goes with the Delete key too, picked from its label; the box's ✕ is gone with it.
		await press(card0);
		await press(label);
		await expect(inspector).toHaveAttribute('data-subject', 'transition');
		await page.keyboard.press('Delete');
		await expect(label).toHaveCount(0);
		await expect(card0.getByTestId('state-remove')).toHaveCount(0);

		// The playhead's group is the machine's: listed locked whole, like a device's.
		const groups = (await rawCall(page, 'session state')).result.variable_groups;
		expect(groups.playhead0.machine).toBe('machine0');

		// The tabs' ＋ starts a second machine and shows it; its ✕ removes it and the first comes back.
		// The last tab's ✕ leaves a fresh empty machine, as a new panel on a new patch gets one.
		await press(tabs.locator('.ui-tab-add'));
		await expect(panel).toHaveAttribute('data-machine', 'machine1');
		await expect(tabs.locator('[role=tab]')).toHaveCount(2);
		await press(tabs.locator('.ui-tab.active .ui-tab-close'));
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		await expect(card0).toBeVisible();
		await press(tabs.locator('.ui-tab.active .ui-tab-close'));
		await expect(card0).toHaveCount(0);
		await expect(tabs.locator('[role=tab]')).toHaveCount(1);
		await expect(panel).toHaveAttribute('data-machine', 'machine0');

		await expectIntact(page, 'the machine panel');
	} finally {
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});
