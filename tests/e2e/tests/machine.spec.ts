// One state machine, built with the pointer the project gives: two boxes from a double-click or a
// held finger, a transition dragged from one box's edge to the other, and everything else in the
// inspector of the selected state — its playheads, its attributes and their envelopes, its
// decision and its transitions — and the dot that crosses when a transition is fired. Runs on
// the desktop and the phone, and sweeps the scene.

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
		// A new card is selected, and its inspector takes the right side, or the bottom on a phone: the
		// cards sit in the canvas it leaves bare, apart on a diagonal, and a tap on the bare corner
		// closes it between the two. A card is wider than half a phone, so the phone zooms out first.
		const corner = { x: box.x + box.width * 0.55, y: box.y + box.height * 0.04 };
		const tapCorner = async (): Promise<void> => {
			if (hasTouch) await page.touchscreen.tap(corner.x, corner.y);
			else await page.mouse.click(corner.x, corner.y);
		};
		if (hasTouch) for (let i = 0; i < 3; i++) await press(panel.locator('.svelte-flow__controls-zoomout'));
		await addState({ x: box.x + box.width * 0.35, y: box.y + box.height * 0.12 }, card0);
		await tapCorner();
		await expect(page.getByTestId('auto-side-panel')).not.toHaveClass(/open/);
		await addState({ x: box.x + box.width * 0.75, y: box.y + box.height * 0.3 }, card1);

		// A transition dragged from one box's edge into the other is selected: the inspector shows
		// the state it leaves, with the transition's row open on its settings.
		await drag(page, hasTouch, card0, card1);
		const label = page.getByTestId('transition-t1');
		// A new box draws evenly among its transitions out; this one holds, so only a fire moves it.
		await expect(label).toHaveText('100% | manual');
		await rawCall(page, 'machine state edit', { machine: 'machine0', name: 'state0', noop: 1, weights: { t1: 0 } });
		await expect(label).toHaveText('manual');
		const inspector = page.getByTestId('machine-inspector');
		await expect(inspector).toHaveAttribute('data-count', '1');
		await expect(inspector.getByTestId('state-name')).toHaveText('state0');
		await expect(inspector).toHaveAttribute('data-open', 't1');
		await expect(inspector.getByTestId('transition-row-t1').getByTestId('machine-transition')).toBeVisible();

		// A box selected shows its name and the transition out of it, its row closed; the bare
		// canvas shows no inspector.
		const name0 = card0.getByTestId('state-card-name');
		const name1 = card1.getByTestId('state-card-name');
		await press(name0);
		await expect(inspector).toHaveAttribute('data-open', '');
		await expect(inspector.getByTestId('transitions-out').getByTestId('transition-row-t1')).toBeVisible();
		await expect(inspector.getByTestId('machine-transition')).toHaveCount(0);
		await expect(inspector.getByTestId('transitions-in').getByTestId('transition-row-t1')).toHaveCount(0);
		await tapCorner();
		await expect(page.getByTestId('auto-side-panel')).not.toHaveClass(/open/);

		// A playhead is born in the selected box, from its inspector, and its dot sits on that box's
		// top edge.
		await press(name0);
		await press(inspector.getByTestId('machine-add-playhead'));
		const dot = page.getByTestId('playhead-dot');
		await expect(dot).toHaveAttribute('data-state', 'state0');
		await expect(inspector.getByTestId('playhead-state')).toHaveText('state0');
		await tapCorner();
		await expect.poll(() => onTopEdge(dot, card0)).toBe(true);

		// The label is gone while nothing of it is selected; a box of the transition brings it back, and
		// a tap on it opens its row. The fire button moves the playhead, and the dot comes to rest on
		// the target box.
		await expect(label).toHaveCount(0);
		await press(name1);
		await press(label);
		await expect(inspector).toHaveAttribute('data-open', 't1');
		await press(inspector.getByTestId('transition-fire'));
		await expect(dot).toHaveAttribute('data-state', 'state1');
		await expect(dot).toHaveAttribute('data-flying', 'false');
		await expect.poll(() => onTopEdge(dot, card1)).toBe(true);
		await tapCorner();
		await press(name0);
		await expect(inspector.getByTestId('playhead-state')).toHaveText('state1');
		await tapCorner();

		// The seed rides the bar; the bar's tabs are the switch between machines.
		await expect(panel.getByTestId('machine-seed')).toHaveValue('0');
		const tabs = panel.getByTestId('machine-tabs');
		await expect(tabs.locator('[role=tab][aria-selected="true"]')).toHaveText('machine0');

		// A name typed into the selected box's inspector is a new attribute, set there and drawn as a
		// param: a number's slider. A cleared value keeps its slider, and a turn sets it again.
		const machineState = async (): Promise<any> => (await rawCall(page, 'session state')).result.machines.machine0;
		await press(name1);
		const newName = inspector.getByTestId('attribute-name-new');
		await newName.fill('gain');
		await newName.press('Enter');
		const gain = inspector.getByTestId('state-attribute-gain');
		await expect(gain).toHaveAttribute('data-set', 'true');
		await expect(newName).toHaveValue('');
		await press(gain.getByTestId('state-clear'));
		await expect(gain).toHaveAttribute('data-set', 'false');
		await gain.getByTestId('param-slider').locator('input').fill('0.7');
		await expect(gain).toHaveAttribute('data-set', 'true');
		await expect.poll(async () => (await machineState()).states.state1.values?.gain).toBeCloseTo(0.7);
		// A set number may land by an envelope: its knobs under the value, born as a short pluck; a
		// turn of the attack writes the four, and the switch takes the envelope away again.
		await expect(gain).toHaveAttribute('data-envelope', 'false');
		await press(gain.getByTestId('state-envelope'));
		await expect(gain).toHaveAttribute('data-envelope', 'true');
		const envelope = gain.getByTestId('state-attribute-envelope').getByTestId('param-envelope');
		await expect(envelope).toBeVisible();
		await expect.poll(async () => (await machineState()).states.state1.envelopes?.gain).toEqual([0.01, 0.1, 1, 0.3]);
		await envelope.locator('[data-stage="attack"] [role=slider]').focus();
		await page.keyboard.press('ArrowUp');
		await expect.poll(async () => (await machineState()).states.state1.envelopes?.gain[0]).toBeGreaterThan(0.01);
		await press(gain.getByTestId('state-envelope'));
		await expect(gain).toHaveAttribute('data-envelope', 'false');
		await expect.poll(async () => (await machineState()).states.state1.envelopes?.gain).toBeUndefined();
		await tapCorner();

		// Both boxes selected: the inspector writes a value to each as one step, and defines the
		// attribute for every state under its pencil.
		const pick = async (l: Locator): Promise<void> => (hasTouch ? l.tap() : l.click({ modifiers: ['Shift'] }));
		const multi = async (): Promise<void> => {
			if (!hasTouch) return;
			await press(page.getByTestId('topbar-overflow'));
			await press(page.getByText('Multi-select mode'));
		};
		await press(name1);
		await multi();
		await pick(name0);
		await expect(inspector).toHaveAttribute('data-count', '2');
		const both = inspector.getByTestId('state-attribute-gain');
		await expect(both).toHaveAttribute('data-mixed', 'true');
		await both.getByTestId('param-slider').locator('input').fill('0.3');
		await expect(both).toHaveAttribute('data-mixed', 'false');
		await expect.poll(async () => Object.values((await machineState()).states).map((s: any) => s.values?.gain)).toEqual([0.3, 0.3]);
		await press(page.getByTestId('topbar-undo'));
		await expect.poll(async () => Object.values((await machineState()).states).map((s: any) => s.values?.gain)).toEqual([undefined, 0.7]);
		// A new kind takes the set value with it, read as a param reads it, and the widgets and the
		// kind's settings follow: a text has no range and no integer switch.
		await press(both.getByTestId('attribute-edit'));
		const kind = inspector.getByTestId('attribute-gain');
		await expect(kind).toHaveAttribute('data-face', 'number');
		await expect(kind.locator('[aria-label="integer"]')).toHaveCount(1);
		await kind.getByTestId('attribute-kind').locator('select').selectOption('text');
		await expect(kind).toHaveAttribute('data-face', 'text');
		await expect.poll(async () => (await machineState()).states.state1.values?.gain).toBe('0.7');
		await expect(kind.locator('[aria-label="integer"], [aria-label="min"]')).toHaveCount(0);
		await expect(both.locator('.slot-head').getByTestId('param-text')).toBeVisible();
		await press(both.getByTestId('state-clear'));
		await expect(both).toHaveAttribute('data-set', 'false');
		await kind.getByTestId('attribute-kind').locator('select').selectOption('toggle');
		await expect(kind).toHaveAttribute('data-face', 'toggle');
		await expect(both.locator('.slot-head').getByTestId('param-toggle')).toBeVisible();
		await multi();
		await tapCorner();

		await page.screenshot({ path: testInfo.outputPath('machine.png') });

		// A box decides each interval, set in its inspector: a drawn share moves the others with it
		// so they sum to one, and its label carries it. The interval is typed; the mode toggle
		// switches to a sequence, and the card's line says so.
		const decision = async (): Promise<{ noop: number; weights: Record<string, number>; interval: number; mode: string }> =>
			(await rawCall(page, 'session state')).result.machines.machine0.states.state0;
		await expect(card0).toHaveAttribute('data-mode', 'random');
		await press(name0);
		await inspector.getByTestId('state-share-t1').locator('input').fill('0.6');
		await expect.poll(async () => (await decision()).weights.t1).toBeCloseTo(0.6);
		expect((await decision()).noop).toBeCloseTo(0.4);
		await expect(inspector.getByTestId('state-share-wait')).toContainText('40%');
		await expect(label).toHaveText('60% | manual');
		const interval = inspector.getByTestId('state-interval').getByTestId('param-number');
		await interval.fill('1.05');
		await interval.press('Enter');
		await expect.poll(async () => (await decision()).interval).toBeCloseTo(1.05);
		await press(inspector.getByTestId('state-mode-sequence'));
		await expect(card0).toHaveAttribute('data-mode', 'sequence');
		await expect(card0.getByTestId('state-card-summary')).toContainText('seq · 1.05s');
		await expect(inspector.getByTestId('state-share-t1')).toContainText('1 → state1');
		await expect(label).toHaveText('#1 | manual');
		await tapCorner();

		// A transition back the other way runs beside the first, its label apart from the first's, and
		// either label selects its own. The labels show once a box of theirs is selected.
		await rawCall(page, 'machine transition add', { machine: 'machine0', from: 'state1', to: 'state0', triggers: [{ kind: 'manual' }] });
		const back = page.getByTestId('transition-t2');
		await expect(back).toHaveCount(0);
		await press(name0);
		await expect(back).toBeVisible();
		const [l1, l2] = await Promise.all([label.boundingBox(), back.boundingBox()]);
		expect(l1!.x + l1!.width <= l2!.x || l2!.x + l2!.width <= l1!.x || l1!.y + l1!.height <= l2!.y || l2!.y + l2!.height <= l1!.y, `the two labels do not overlap: ${JSON.stringify(l1)} ${JSON.stringify(l2)}`).toBe(true);
		// The way back, picked by its label, is read through the box it leaves, its row open: its
		// duration is set there, and the Delete key takes the selected transition.
		await press(back);
		await expect(inspector.getByTestId('state-name')).toHaveText('state1');
		await expect(inspector).toHaveAttribute('data-open', 't2');
		await inspector.getByTestId('transition-duration').getByTestId('param-slider').locator('input').fill('0.5');
		await expect.poll(async () => (await machineState()).transitions.t2.duration).toBeCloseTo(0.5);
		// The keys are the slider's while it has the focus; a tap on the label gives them back to the canvas.
		await press(back);
		await expect(page.locator('.svelte-flow__edge.selected')).toHaveAttribute('data-id', /t2/);
		await page.keyboard.press('Delete');
		await expect(back).toHaveCount(0);

		// The first goes with the Delete key too, picked from its label.
		await press(name0);
		await press(label);
		await expect(inspector).toHaveAttribute('data-open', 't1');
		await page.keyboard.press('Delete');
		await expect(label).toHaveCount(0);
		// A transition is added from the inspector too, to a state picked from the list, and its
		// open row's delete button takes it away. The manager mints the first free id again.
		await press(name0);
		await inspector.getByTestId('transition-add-to').locator('select').selectOption('state1');
		await expect.poll(async () => Object.keys((await machineState()).transitions)).toEqual(['t1']);
		const row = inspector.getByTestId('transition-row-t1');
		await expect(row).toBeVisible();
		await press(row.locator('.ui-disclosure-summary'));
		await press(row.getByTestId('transition-remove'));
		await expect(row).toHaveCount(0);

		// The zoom cluster is never under the inspector pane: the pane takes its room from the canvas.
		// The cluster shows while the pointer is in the panel, and always on a touch screen.
		await press(name1);
		await expect(inspector).toHaveAttribute('data-subject', 'state');
		const cluster = panel.locator('.svelte-flow__controls');
		if (!hasTouch) await page.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.5);
		await expect(cluster).toHaveCSS('opacity', '1');
		const [c, p] = await Promise.all([cluster.boundingBox(), page.getByTestId('auto-side-panel').boundingBox()]);
		expect(c!.x + c!.width <= p!.x || c!.y + c!.height <= p!.y, `the cluster sits clear of the pane: ${JSON.stringify(c)} ${JSON.stringify(p)}`).toBe(true);
		// Its ◇ is `machine arrange`: every card lands on the grid, clear of the others, as one step.
		const states = async (): Promise<Record<string, { pos: [number, number] }>> =>
			(await rawCall(page, 'session state')).result.machines.machine0.states;
		const before = await states();
		await press(cluster.getByTestId('arrange'));
		await expect.poll(async () => Object.values(await states()).every((s) => s.pos[0] % 24 === 0 && s.pos[1] % 24 === 0)).toBe(true);
		const ringed = await states();
		expect(ringed.state0.pos).not.toEqual(ringed.state1.pos);
		await press(page.getByTestId('topbar-undo'));
		await expect.poll(async () => (await states()).state0.pos).toEqual(before.state0.pos);

		// The playhead's group is the machine's: listed locked whole, like a device's.
		const groups = (await rawCall(page, 'session state')).result.variable_groups;
		expect(groups.playhead0.machine).toBe('machine0');

		// The tabs' ＋ starts a second machine and shows it; its ✕ removes it and the first comes back.
		// The last tab's ✕ leaves a fresh empty machine, as a new panel on a new patch gets one.
		await press(tabs.locator('.ui-tab-add'));
		await expect(panel).toHaveAttribute('data-machine', 'machine1');
		await expect(tabs.locator('[role=tab]')).toHaveCount(2);
		// The ＋ was one step too: undo takes the machine and the panel's place on it back together.
		await press(page.getByTestId('topbar-undo'));
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		await expect(tabs.locator('[role=tab]')).toHaveCount(1);
		await press(page.getByTestId('topbar-redo'));
		await expect(panel).toHaveAttribute('data-machine', 'machine1');
		await expect(tabs.locator('[role=tab]')).toHaveCount(2);
		await press(tabs.locator('.ui-tab.active .ui-tab-close'));
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		await expect(card0).toBeVisible();
		await press(tabs.locator('.ui-tab.active .ui-tab-close'));
		await expect(card0).toHaveCount(0);
		await expect(tabs.locator('[role=tab]')).toHaveCount(1);
		await expect(panel).toHaveAttribute('data-machine', 'machine1');
		// That was ONE step: undo brings the old machine back whole and the fresh one goes; redo repeats it.
		await press(page.getByTestId('topbar-undo'));
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		await expect(card0).toBeVisible();
		await expect(tabs.locator('[role=tab]')).toHaveCount(1);
		await press(page.getByTestId('topbar-redo'));
		await expect(panel).toHaveAttribute('data-machine', 'machine1');
		await expect(card0).toHaveCount(0);

		await expectIntact(page, 'the machine panel');
	} finally {
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});
