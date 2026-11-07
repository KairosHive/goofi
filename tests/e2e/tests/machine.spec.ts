// One state machine, built with the pointer the project gives: two boxes from a double-click or a
// held finger, a transition dragged from one box's edge to the other, and everything else in the
// score beside the canvas — the states as columns, the attributes as rows, every cell a slider a
// drag paints across, a column's cog for its decision, playheads and transitions, a row's for the
// attribute's definition and curve, a cell's for the classic widget and the envelope, an arc's
// for the transition — and the dot that crosses when a transition is fired. Runs on the desktop
// and the phone, and sweeps the scene. A second test, desktop only, wires groups: a fan out of a
// selection, a fan into one, all to all, a chain across boxes, several states at once, and a
// copy pasted. A third drops a box on a node's param row to map it, completes an expression with
// Tab in a param's editor and in a condition's, and sends the Delete key to the active panel alone.

import { test, expect, type Locator, type Page } from '@playwright/test';
import { closeSplit, restorePanelType, splitRight, waitForApp } from '../lib/app';
import { addNode, nodeParams, nodes, selectNode, waitForNoNode, waitForNode } from '../lib/goofi';
import { expectIntact } from '../lib/invariants';
import { backendDoc, rawCall } from '../lib/raw';
import { swipe, touchSession } from '../lib/touch';

/** Whether `dot`'s centre sits on `card`'s top edge. */
async function onTopEdge(dot: Locator, card: Locator): Promise<boolean> {
	const [d, c] = await Promise.all([dot.boundingBox(), card.boundingBox()]);
	if (!d || !c) return false;
	const cx = d.x + d.width / 2;
	const cy = d.y + d.height / 2;
	return cx >= c.x && cx <= c.x + c.width && Math.abs(cy - c.y) <= 2;
}

/** A drag from `start` to `end` with the project's pointer. A finger comes to rest before it
 * lifts, or Chromium reads a fling and eats the tap that follows. */
async function dragPoints(page: Page, hasTouch: boolean, start: { x: number; y: number }, end: { x: number; y: number }): Promise<void> {
	if (hasTouch) return swipe(page, start, end);
	await page.mouse.move(start.x, start.y);
	await page.mouse.down();
	await page.mouse.move(end.x, end.y, { steps: 8 });
	await page.mouse.up();
}

/** A drag from a point on `from`'s right edge to a point well off `to`'s centre. */
async function drag(page: Page, hasTouch: boolean, from: Locator, to: Locator): Promise<void> {
	const [a, b] = await Promise.all([from.boundingBox(), to.boundingBox()]);
	await dragPoints(page, hasTouch, { x: a!.x + a!.width - 3, y: a!.y + a!.height / 2 }, { x: b!.x + b!.width * 0.25, y: b!.y + b!.height * 0.4 });
}

/** A press on a cell at `fraction` of its height from the bottom: what a slider's value is. */
async function paint(page: Page, hasTouch: boolean, cell: Locator, fraction: number, to?: Locator): Promise<void> {
	const a = (await cell.boundingBox())!;
	const start = { x: a.x + a.width / 2, y: a.y + a.height * (1 - fraction) };
	const b = to ? (await to.boundingBox())! : a;
	await dragPoints(page, hasTouch, start, { x: b.x + b.width / 2, y: b.y + b.height * (1 - fraction) });
}

test('a machine is built by gesture, scored as columns and rows, and its playhead crosses on a tap', async ({ page, hasTouch }, testInfo) => {
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

		// A machine panel on a patch without a machine starts one, and shows it: the canvas, and
		// the score beside it (under it on a phone), empty but for its own + for a first state.
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'machine');
		});
		const panel = page.getByTestId('machine-panel');
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		const score = page.getByTestId('machine-inspector');
		await expect(score).toHaveAttribute('data-order', '');
		await expect(page.getByTestId('auto-side-panel')).toHaveClass(/open/);

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
		/** A tap on the bare top of the canvas, between the boxes: nothing is selected after it. */
		const bare = async (): Promise<void> => {
			const at = { x: box.x + box.width * 0.55, y: box.y + box.height * 0.04 };
			if (hasTouch) await page.touchscreen.tap(at.x, at.y);
			else await page.mouse.click(at.x, at.y);
			await expect(score).toHaveAttribute('data-count', '0');
		};
		// The cards sit apart on a diagonal of the canvas the score leaves; a card is wider than
		// half a phone, so the phone zooms out first.
		if (hasTouch) for (let i = 0; i < 3; i++) await press(panel.locator('.svelte-flow__controls-zoomout'));
		await addState({ x: box.x + box.width * 0.3, y: box.y + box.height * 0.15 }, card0);
		await expect(score).toHaveAttribute('data-order', 'state0');
		await addState({ x: box.x + box.width * 0.7, y: box.y + box.height * 0.45 }, card1);
		const column0 = score.getByTestId('column-state0');
		const column1 = score.getByTestId('column-state1');
		await expect(column1).toBeVisible();

		// A transition dragged from one box's edge into the other is selected: its arc's chip in the
		// score's lane is picked, and the canvas label carries the state's decision.
		await drag(page, hasTouch, card0, card1);
		const label = page.getByTestId('transition-t1');
		// A new box draws evenly among its transitions out; this one holds, so only a fire moves it.
		await expect(label).toHaveText('100%');
		await rawCall(page, 'machine state edit', { machine: 'machine0', name: 'state0', noop: 1, weights: { t1: 0 } });
		await expect(label).toHaveText('0%');
		await expect(score).toHaveAttribute('data-open', 't1');
		const arc = score.getByTestId('arc-t1');
		await expect(arc).toHaveClass(/picked/);
		await expect(arc).toHaveText('0%');
		// The two columns are joined by the transition, so a thin edge parts them.
		await expect(score).toHaveAttribute('data-order', 'state0 state1');
		await expect(column0).not.toHaveClass(/bold/);

		// A box's name selects it: its column lights in the score, and the score's cog opens the
		// state's settings with the transition out of it; Escape closes them.
		const name0 = card0.getByTestId('state-card-name');
		const name1 = card1.getByTestId('state-card-name');
		await press(name0);
		await expect(column0).toHaveAttribute('data-selected', 'true');
		await expect(column1).toHaveAttribute('data-selected', 'false');
		await expect(score).toHaveAttribute('data-count', '1');
		const popover = page.getByTestId('score-popover');
		await press(column0.getByTestId('column-settings'));
		await expect(popover).toHaveAttribute('data-kind', 'state');
		await expect(popover.getByTestId('state-name')).toHaveText('state0');
		await expect(popover.getByTestId('transitions-out').getByTestId('transition-row-t1')).toBeVisible();
		await expect(popover.getByTestId('transitions-in').getByTestId('transition-row-t1')).toHaveCount(0);

		// A playhead is born in the state, from its settings: its dot sits on the box's top edge and
		// at the head of the column.
		await press(popover.getByTestId('machine-add-playhead'));
		const dot = page.getByTestId('playhead-dot');
		await expect(dot).toHaveAttribute('data-state', 'state0');
		await expect(popover.getByTestId('playhead-state')).toHaveText('state0');
		await page.keyboard.press('Escape');
		await expect(popover).toHaveCount(0);
		await expect(column0.getByTestId('score-dot')).toHaveAttribute('data-playhead', 'playhead0');
		await expect.poll(() => onTopEdge(dot, card0)).toBe(true);

		// The label is gone while nothing of it is selected; a box of the transition brings it back.
		// The arc's chip opens the transition's settings, whose fire button moves the playhead: the
		// dot comes to rest on the target box, and under the target column.
		await bare();
		await expect(label).toHaveCount(0);
		await press(name1);
		await expect(label).toBeVisible();
		await press(arc);
		await expect(popover).toHaveAttribute('data-kind', 'transition');
		await expect(score).toHaveAttribute('data-open', 't1');
		await press(popover.getByTestId('transition-fire'));
		await expect(dot).toHaveAttribute('data-state', 'state1');
		await expect(dot).toHaveAttribute('data-flying', 'false');
		await expect.poll(() => onTopEdge(dot, card1)).toBe(true);
		await expect(column1.getByTestId('score-dot')).toHaveCount(1);
		await page.keyboard.press('Escape');

		// The seed rides the bar; the bar's tabs are the switch between machines.
		await expect(panel.getByTestId('machine-seed')).toHaveValue('0');
		const tabs = panel.getByTestId('machine-tabs');
		await expect(tabs.locator('[role=tab][aria-selected="true"]')).toHaveText('machine0');

		// A name typed at the score's foot is a new attribute: a row, set in the selected state and
		// unset in the other. A press on a cell sets its value where the pointer is; a drag across
		// the columns paints each; unset in its last state, the attribute leaves the machine.
		const machineState = async (): Promise<any> => (await rawCall(page, 'session state')).result.machines.machine0;
		await press(name1);
		const newName = score.getByTestId('attribute-name-new');
		await newName.fill('gain');
		await newName.press('Enter');
		const row = score.getByTestId('row-gain');
		const gain0 = score.getByTestId('cell-gain-state0');
		const gain1 = score.getByTestId('cell-gain-state1');
		await expect(gain1).toHaveAttribute('data-set', 'true');
		await expect(gain0).toHaveAttribute('data-set', 'false');
		await expect(newName).toHaveValue('');
		await paint(page, hasTouch, gain1, 0.7);
		await expect.poll(async () => (await machineState()).states.state1.values?.gain).toBeGreaterThan(0.6);
		expect((await machineState()).states.state1.values.gain).toBeLessThan(0.8);
		await paint(page, hasTouch, gain1, 0.3, gain0);
		await expect(gain0).toHaveAttribute('data-set', 'true');
		await expect.poll(async () => Object.values((await machineState()).states).map((s: any) => Math.round(s.values?.gain * 10))).toEqual([3, 3]);
		// That paint was ONE step: undo takes both cells back.
		await press(page.getByTestId('topbar-undo'));
		await expect(gain0).toHaveAttribute('data-set', 'false');
		await expect.poll(async () => Math.round((await machineState()).states.state1.values?.gain * 10)).toBe(7);
		// A cell's cog: the classic widget, the envelope, and unset.
		await press(gain1.getByTestId('cell-settings'));
		await expect(popover).toHaveAttribute('data-kind', 'cell');
		await popover.getByTestId('param-slider').locator('input').fill('0.5');
		await expect.poll(async () => (await machineState()).states.state1.values?.gain).toBeCloseTo(0.5);
		// A set number may land by an envelope: born as a short pluck, its bar opens the knobs with
		// a number under each; a typed attack writes the four, and the switch takes it away again.
		await expect(gain1).toHaveAttribute('data-envelope', 'false');
		await press(popover.getByTestId('state-envelope'));
		await expect(gain1).toHaveAttribute('data-envelope', 'true');
		await expect.poll(async () => (await machineState()).states.state1.envelopes?.gain).toEqual([0.01, 0.1, 1, 0.3]);
		await press(popover.getByTestId('state-attribute-envelope').getByTestId('param-envelope').locator('button'));
		const attack = page.getByTestId('envelope-attack');
		await expect(attack).toBeVisible();
		await attack.fill('0.2');
		await attack.press('Enter');
		await expect.poll(async () => (await machineState()).states.state1.envelopes?.gain[0]).toBeCloseTo(0.2);
		await page.keyboard.press('Escape');
		await expect(attack).toHaveCount(0);
		await expect(popover).toHaveAttribute('data-kind', 'cell');
		await press(popover.getByTestId('state-envelope'));
		await expect(gain1).toHaveAttribute('data-envelope', 'false');
		await expect.poll(async () => (await machineState()).states.state1.envelopes?.gain).toBeUndefined();
		await press(popover.getByTestId('cell-unset'));
		await expect(row).toHaveCount(0);
		await expect(popover).toHaveCount(0);
		await expect.poll(async () => (await machineState()).attributes?.gain).toBeUndefined();

		// Both columns selected: a paint in one lands in both as one step, and the row's cog defines
		// the attribute for every state. A new kind takes the set value with it, read as a param
		// reads it, and the widgets and the kind's settings follow: a text has no range and no
		// integer switch, and its cell shows the text itself.
		const pick = async (l: Locator): Promise<void> => (hasTouch ? l.tap() : l.click({ modifiers: ['Shift'] }));
		const multi = async (): Promise<void> => {
			if (!hasTouch) return;
			await press(page.getByTestId('topbar-overflow'));
			await press(page.getByText('Multi-select mode'));
		};
		await newName.fill('gain');
		await newName.press('Enter');
		await expect(gain1).toHaveAttribute('data-set', 'true');
		await multi();
		await pick(column0.getByTestId('column-pick'));
		await expect(score).toHaveAttribute('data-count', '2');
		await paint(page, hasTouch, gain1, 0.3);
		await expect.poll(async () => Object.values((await machineState()).states).map((s: any) => Math.round(s.values?.gain * 10))).toEqual([3, 3]);
		await press(page.getByTestId('topbar-undo'));
		await expect.poll(async () => Object.values((await machineState()).states).map((s: any) => s.values?.gain)).toEqual([undefined, 0]);
		// A cell's widget writes every selected column too.
		await press(gain1.getByTestId('cell-settings'));
		await expect(popover.getByText('2 states')).toBeVisible();
		await popover.getByTestId('param-slider').locator('input').fill('0.5');
		await expect.poll(async () => Object.values((await machineState()).states).map((s: any) => s.values?.gain)).toEqual([0.5, 0.5]);
		await page.keyboard.press('Escape');
		await expect(popover).toHaveCount(0);
		await press(row.getByTestId('row-settings'));
		await expect(popover).toHaveAttribute('data-kind', 'attribute');
		const kind = popover.getByTestId('attribute-gain');
		await expect(kind).toHaveAttribute('data-face', 'number');
		await expect(kind.locator('[aria-label="integer"]')).toHaveCount(1);
		// The curve is the attribute's: a preset picked from the strip, or a custom bezier whose
		// handles are typed, seeded from the shape of the preset it leaves.
		await press(popover.getByTestId('attribute-curve').locator('button'));
		await press(page.getByTestId('curve-preset').getByText('smooth'));
		await expect.poll(async () => (await machineState()).attributes.gain.curve).toBe('smooth');
		await press(page.getByTestId('curve-preset').getByText('custom'));
		await expect.poll(async () => (await machineState()).attributes.gain.curve).toEqual({ bezier: [0.33, 0, 0.67, 1] });
		const x1 = page.getByTestId('curve-x1');
		await x1.fill('0.5');
		await x1.press('Enter');
		await expect.poll(async () => (await machineState()).attributes.gain.curve.bezier[0]).toBeCloseTo(0.5);
		await page.keyboard.press('Escape');
		await expect(page.getByTestId('curve-preset')).toHaveCount(0);
		await kind.getByTestId('attribute-kind').locator('select').selectOption('text');
		await expect(kind).toHaveAttribute('data-face', 'text');
		await expect.poll(async () => (await machineState()).states.state1.values?.gain).toBe('0.5');
		await expect(kind.locator('[aria-label="integer"], [aria-label="min"]')).toHaveCount(0);
		await expect(gain1.getByTestId('cell-text')).toHaveText('0.5');
		await kind.getByTestId('attribute-kind').locator('select').selectOption('toggle');
		await expect(kind).toHaveAttribute('data-face', 'toggle');
		await expect(gain1.getByTestId('cell-readout')).toHaveText('1');
		await page.keyboard.press('Escape');
		// Unset through a selected column's cog unsets every selected column: the attribute is gone.
		await press(gain1.getByTestId('cell-settings'));
		await expect(popover.getByText('2 states')).toBeVisible();
		await press(popover.getByTestId('cell-unset'));
		await expect(row).toHaveCount(0);
		await expect.poll(async () => (await machineState()).attributes?.gain).toBeUndefined();
		await multi();
		await press(name0);
		await expect(score).toHaveAttribute('data-count', '1');

		await page.screenshot({ path: testInfo.outputPath('machine.png') });

		// A box decides each interval, set in its column's settings: a share drawn on a transition's
		// row moves the others with it so they sum to one, and the arc's chip and the label carry it.
		// The interval is typed, and dragged on the column's edge; the mode toggle switches to a
		// sequence, and the card's line, the row and the chip say so.
		const decision = async (): Promise<{ noop: number; weights: Record<string, number>; interval: number; mode: string }> =>
			(await rawCall(page, 'session state')).result.machines.machine0.states.state0;
		await expect(card0).toHaveAttribute('data-mode', 'random');
		await press(column0.getByTestId('column-settings'));
		await expect(popover).toHaveAttribute('data-kind', 'state');
		const shareT1 = popover.getByTestId('transition-row-t1').getByTestId('param-number');
		await shareT1.fill('0.6');
		await shareT1.press('Enter');
		await expect.poll(async () => (await decision()).weights.t1).toBeCloseTo(0.6);
		expect((await decision()).noop).toBeCloseTo(0.4);
		await expect(popover.getByTestId('state-share-wait').getByTestId('param-number')).toHaveValue('0.4');
		await expect(label).toHaveText('60%');
		await expect(arc).toHaveText('60%');
		const interval = popover.getByTestId('state-interval').getByTestId('param-number');
		await interval.fill('1.05');
		await interval.press('Enter');
		await expect.poll(async () => (await decision()).interval).toBeCloseTo(1.05);
		// The interval is a param: its source switch seeds an expression from the value, and back.
		await press(popover.getByTestId('state-interval').getByRole('button', { name: 'interval', exact: true }));
		await press(popover.getByTestId('state-interval').getByTestId('param-mode-expression'));
		await expect.poll(async () => (await rawCall(page, 'session state')).result.machines.machine0.states.state0.interval_expression).toBe('1.05');
		await press(popover.getByTestId('state-interval').getByTestId('param-mode-constant'));
		await expect.poll(async () => (await rawCall(page, 'session state')).result.machines.machine0.states.state0.interval_expression).toBeUndefined();
		await press(popover.getByTestId('state-mode-sequence'));
		await expect(card0).toHaveAttribute('data-mode', 'sequence');
		await expect(card0.getByTestId('state-card-summary')).toContainText('seq');
		await expect(card0.getByTestId('state-card-summary')).toContainText('1.05s');
		await expect(popover.getByTestId('transition-row-t1').getByTestId('param-number')).toHaveCount(0);
		await expect(popover.getByTestId('transition-row-t1')).toContainText('#1');
		await expect(label).toHaveText('#1');
		await expect(arc).toHaveText('#1');
		// A condition typed into the open row rides the row's summary, and the canvas label stays the
		// decision's; the condition's own editor completes `sm(` with the machine's states.
		await press(popover.getByTestId('transition-row-t1').locator('.ui-field-summary'));
		const condition = popover.getByTestId('transition-condition');
		await expect(condition).toBeVisible();
		await press(condition);
		await page.keyboard.type("sm('");
		await expect(page.getByTestId('expr-window')).toContainText('state1');
		await page.keyboard.press('Escape');
		await page.keyboard.type("state1').residents > 0");
		await page.keyboard.press('Enter');
		await expect.poll(async () => (await machineState()).transitions.t1.condition).toBe("sm('state1').residents > 0");
		await expect(popover.getByTestId('transition-row-t1')).toContainText("if sm('state1').residents > 0");
		await expect(label).toHaveText('#1');
		await page.keyboard.press('Escape');
		await expect(popover).toHaveCount(0);
		// The column's edge, dragged right, lengthens the interval: the column grows with it.
		const before = (await column0.boundingBox())!;
		const edge = (await column0.getByTestId('column-edge').boundingBox())!;
		await dragPoints(page, hasTouch, { x: edge.x + edge.width / 2, y: edge.y + edge.height / 2 }, { x: edge.x + edge.width / 2 + 40, y: edge.y + edge.height / 2 });
		await expect.poll(async () => (await decision()).interval).toBeGreaterThan(1.05);
		await expect.poll(async () => (await column0.boundingBox())!.width).toBeGreaterThan(before.width);

		// A transition back the other way runs beside the first, its label apart from the first's, and
		// either label selects its own. The labels show once a box of theirs is selected.
		await bare();
		await rawCall(page, 'machine transition add', { machine: 'machine0', from: 'state1', to: 'state0' });
		const back = page.getByTestId('transition-t2');
		await expect(back).toHaveCount(0);
		await press(name0);
		await expect(back).toBeVisible();
		const [l1, l2] = await Promise.all([label.boundingBox(), back.boundingBox()]);
		expect(l1!.x + l1!.width <= l2!.x || l2!.x + l2!.width <= l1!.x || l1!.y + l1!.height <= l2!.y || l2!.y + l2!.height <= l1!.y, `the two labels do not overlap: ${JSON.stringify(l1)} ${JSON.stringify(l2)}`).toBe(true);
		// The way back, picked by its label, lights its arc; the arc's chip opens its duration, set
		// there, and the Delete key takes the selected transition.
		await press(back);
		await expect(score).toHaveAttribute('data-open', 't2');
		await expect(score.getByTestId('arc-t2')).toHaveClass(/picked/);
		await press(score.getByTestId('arc-t2'));
		await popover.getByTestId('transition-duration').getByTestId('param-slider').locator('input').fill('0.5');
		await expect.poll(async () => (await machineState()).transitions.t2.duration).toBeCloseTo(0.5);
		await expect(score.getByTestId('arc-t2')).toContainText('0.5s');
		await page.keyboard.press('Escape');
		// The keys are the slider's while it has the focus; a tap on the label gives them back to the canvas.
		await press(back);
		await expect(page.locator('.svelte-flow__edge.selected')).toHaveAttribute('data-id', /t2/);
		await page.keyboard.press('Delete');
		await expect(back).toHaveCount(0);

		// The first goes with the Delete key too, picked from its label.
		await press(name0);
		await press(label);
		await expect(score).toHaveAttribute('data-open', 't1');
		await page.keyboard.press('Delete');
		await expect(label).toHaveCount(0);
		await expect(score.getByTestId('arc-t1')).toHaveCount(0);
		// A transition is added from the state's settings too, to a state picked from the list, and
		// its open row's delete button takes it away. The manager mints the first free id again.
		await press(name0);
		await press(column0.getByTestId('column-settings'));
		await press(popover.getByTestId('transition-add'));
		await press(page.locator('.context-menu .item', { hasText: 'state1' }));
		await expect.poll(async () => Object.keys((await machineState()).transitions)).toEqual(['t1']);
		await expect(score.getByTestId('arc-t1')).toBeVisible();
		if (await popover.count() === 0) await press(column0.getByTestId('column-settings'));
		const t1row = popover.getByTestId('transition-row-t1');
		await expect(t1row).toBeVisible();
		await press(t1row.locator('.ui-field-summary'));
		await press(t1row.getByTestId('transition-remove'));
		await expect(t1row).toHaveCount(0);
		await page.keyboard.press('Escape');

		// The zoom cluster is never under the score: the pane takes its room from the canvas. The
		// cluster shows while the pointer is in the panel, and always on a touch screen.
		await press(name1);
		const cluster = panel.locator('.svelte-flow__controls');
		if (!hasTouch) await page.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.5);
		await expect(cluster).toHaveCSS('opacity', '1');
		const [c, p] = await Promise.all([cluster.boundingBox(), page.getByTestId('auto-side-panel').boundingBox()]);
		expect(c!.x + c!.width <= p!.x || c!.y + c!.height <= p!.y, `the cluster sits clear of the pane: ${JSON.stringify(c)} ${JSON.stringify(p)}`).toBe(true);
		// Its ◇ is `machine arrange`: every card lands on the grid, clear of the others, as one step.
		const states = async (): Promise<Record<string, { pos: [number, number] }>> =>
			(await rawCall(page, 'session state')).result.machines.machine0.states;
		const placed = await states();
		await press(cluster.getByTestId('arrange'));
		await expect.poll(async () => Object.values(await states()).every((s) => s.pos[0] % 24 === 0 && s.pos[1] % 24 === 0)).toBe(true);
		const ringed = await states();
		expect(ringed.state0.pos).not.toEqual(ringed.state1.pos);
		await press(page.getByTestId('topbar-undo'));
		await expect.poll(async () => (await states()).state0.pos).toEqual(placed.state0.pos);

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

test('a selection is wired as a group, chained across boxes, added by the handful, and pasted', async ({ page, hasTouch }) => {
	test.skip(hasTouch, 'group wiring is a mouse drag with a modifier and a quick pick; the phone has the menu');
	await page.goto('/');
	await waitForApp(page);
	try {
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'machine');
		});
		const panel = page.getByTestId('machine-panel');
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		const score = page.getByTestId('machine-inspector');
		const machineState = async (): Promise<any> => (await rawCall(page, 'session state')).result.machines.machine0;
		const wires = async (): Promise<string[]> => Object.values((await machineState()).transitions ?? {}).map((t: any) => `${t.from}>${t.to}`).sort();
		const card = (s: string): Locator => page.getByTestId(`state-card-${s}`);
		const name = (s: string): Locator => card(s).getByTestId('state-card-name');
		const fit = (): Promise<void> => panel.locator('.svelte-flow__controls-fitview').click();

		// The cluster's + adds several states at once, chained, as ONE step; a new one is selected.
		await panel.locator('.svelte-flow__pane').hover();
		await page.getByTestId('add-states').click();
		const count = page.getByTestId('add-states-count');
		await count.fill('4');
		await count.press('Enter');
		await page.getByTestId('add-states-go').click();
		await expect(score).toHaveAttribute('data-order', /state0 state1 state2 state3/);
		expect(await wires()).toEqual(['state0>state1', 'state1>state2', 'state2>state3']);
		await page.getByTestId('topbar-undo').click();
		await expect(score).toHaveAttribute('data-order', '');
		await page.getByTestId('topbar-redo').click();
		await expect(card('state3')).toBeVisible();
		await fit();

		// Two selected, a line from one of them to a third: both go to it. A line from a fourth into
		// the pair: it goes to both. A line between the two: all to all is offered, and taken.
		await name('state0').click();
		await name('state1').click({ modifiers: ['Shift'] });
		await expect(score).toHaveAttribute('data-count', '2');
		await drag(page, false, card('state1'), card('state3'));
		await expect.poll(wires).toEqual(['state0>state1', 'state0>state3', 'state1>state2', 'state1>state3', 'state2>state3']);
		await drag(page, false, card('state2'), card('state0'));
		await expect.poll(wires).toEqual(['state0>state1', 'state0>state3', 'state1>state2', 'state1>state3', 'state2>state0', 'state2>state1', 'state2>state3']);
		await drag(page, false, card('state0'), card('state1'));
		const ask = page.getByTestId('quick-pick');
		await expect(ask.getByRole('option', { name: /All to all/ })).toBeVisible();
		await ask.getByRole('option', { name: /All to all/ }).click();
		await expect.poll(wires).toContain('state1>state0');
		await expect(ask).toHaveCount(0);
		// The group's menu wires a pattern too: a ring by position closes the pair the other way.
		await rawCall(page, 'session new');
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'machine');
		});
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		for (const [s, x] of [['a', 0], ['b', 360], ['c', 720]] as const) await rawCall(page, 'machine state add', { machine: 'machine0', name: s, pos: [x, 0] });
		await fit();
		await name('a').click();
		await name('b').click({ modifiers: ['Shift'] });
		await name('c').click({ modifiers: ['Shift'] });
		await name('b').click({ button: 'right' });
		await page.locator('.context-menu .item', { hasText: 'Ring by position' }).click();
		await expect.poll(wires).toEqual(['a>b', 'b>c', 'c>a']);
		// A column is as wide as its interval: two seconds take twice the column one second does.
		await rawCall(page, 'machine state edit', { machine: 'machine0', name: 'a', interval: 2 });
		await rawCall(page, 'machine state edit', { machine: 'machine0', name: 'b', interval: 1 });
		await expect.poll(async () => {
			const [a, b] = await Promise.all([score.getByTestId('column-a').boundingBox(), score.getByTestId('column-b').boundingBox()]);
			return a!.width / b!.width;
		}).toBeCloseTo(2, 0);
		// A box's menu, alone rather than in a group, births a playhead in it.
		await name('c').click();
		await expect(score).toHaveAttribute('data-count', '1');
		await name('c').click({ button: 'right' });
		await page.locator('.context-menu .item', { hasText: 'Add a playhead here' }).click();
		await expect(page.getByTestId('playhead-dot')).toHaveAttribute('data-state', 'c');

		// The chain tool: a line dragged across boxes wires each to the next it crosses.
		await rawCall(page, 'session new');
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'machine');
		});
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		for (const [s, x] of [['a', 0], ['b', 360], ['c', 720]] as const) await rawCall(page, 'machine state add', { machine: 'machine0', name: s, pos: [x, 0] });
		await fit();
		await panel.locator('.svelte-flow__pane').hover();
		await page.getByTestId('chain-tool').click();
		await expect(page.getByTestId('chain-tool')).toHaveAttribute('aria-pressed', 'true');
		const [a, b, c] = await Promise.all([card('a').boundingBox(), card('b').boundingBox(), card('c').boundingBox()]);
		await page.mouse.move(a!.x + a!.width - 3, a!.y + a!.height / 2);
		await page.mouse.down();
		await page.mouse.move(b!.x + b!.width / 2, b!.y + b!.height / 2, { steps: 10 });
		await page.mouse.move(c!.x + c!.width * 0.3, c!.y + c!.height * 0.4, { steps: 10 });
		await page.mouse.up();
		await expect.poll(wires).toEqual(['a>b', 'b>c']);
		await page.getByTestId('chain-tool').click();

		// A copy of two boxes and the wire between them, pasted a cell off: the copies are selected.
		await name('a').click();
		await name('b').click({ modifiers: ['Shift'] });
		await page.keyboard.press('Control+c');
		await page.keyboard.press('Control+v');
		await expect.poll(async () => Object.keys((await machineState()).states).length).toBe(5);
		await expect.poll(wires).toEqual(['a>b', 'b>c', 'state0>state1']);
		await expect(score).toHaveAttribute('data-count', '2');
		const copied = await machineState();
		expect(copied.states.state0.pos).toEqual([48, 48]);
		// Hovering a box lights its column, and a column its box.
		await card('c').hover();
		await expect(score.getByTestId('column-c')).toHaveClass(/lit/);
		await score.getByTestId('column-a').hover();
		await expect(card('a')).toHaveClass(/lit/);
		await expectIntact(page, 'the machine panel after group wiring');
	} finally {
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});

test('a box dropped on a param maps an attribute, Tab completes, and Delete takes the active panel alone', async ({ page, hasTouch }) => {
	test.skip(hasTouch, 'a box onto a row is a mouse drag between two panels; the phone has one');
	await page.goto('/');
	await waitForApp(page);
	let osc: string | null = null;
	try {
		// The editor on the left, a machine on the right: one state that sets two numbers, and a
		// playhead born in it, whose variables a param can read.
		await splitRight(page);
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[1].panelId, 'machine');
		});
		const panel = page.getByTestId('machine-panel');
		await expect(panel).toHaveAttribute('data-machine', 'machine0');
		const num = { type: 'num', vmin: 0, vmax: 1 };
		await rawCall(page, 'machine attribute add', { machine: 'machine0', name: 'gain', value: 0.5, kind: num });
		await rawCall(page, 'machine attribute add', { machine: 'machine0', name: 'rate', value: 2, kind: { ...num, vmax: 10 } });
		await rawCall(page, 'machine state add', { machine: 'machine0', name: 'state0', values: { gain: 0.5, rate: 2 } });
		await rawCall(page, 'machine playhead add', { machine: 'machine0', name: 'beat', start: 'state0' });
		const card = page.getByTestId('state-card-state0');
		await expect(card).toBeVisible();
		const placed = async (): Promise<number[]> => (await backendDoc(page)).machines.machine0.states.state0.pos;
		const before = await placed();

		osc = await addNode(page, 'LFO');
		await waitForNode(page, osc);
		await selectNode(page, osc);
		const row = page.getByTestId('param-field-frequency');
		await row.scrollIntoViewIfNeeded();

		// The box dragged over the row is a mapping, not a move: the row lights, and the drop offers
		// the attributes the state sets; the one picked maps the row to the playhead's element.
		// The box is back where it was.
		const name = card.getByTestId('state-card-name');
		const [a, b] = await Promise.all([name.boundingBox(), row.boundingBox()]);
		await page.mouse.move(a!.x + a!.width / 2, a!.y + a!.height / 2);
		await page.mouse.down();
		await page.mouse.move(a!.x + a!.width / 2 + 30, a!.y + a!.height / 2 + 30, { steps: 4 });
		await page.mouse.move(b!.x + b!.width / 2, b!.y + b!.height / 2, { steps: 12 });
		await expect(row).toHaveClass(/over/);
		await page.mouse.up();
		// The drop asks which attribute, in a list the keyboard walks: the second row is `rate`.
		const pick = page.getByTestId('quick-pick');
		await expect(pick.getByRole('option', { name: 'rate' })).toBeVisible();
		await page.keyboard.press('ArrowDown');
		await expect(pick.getByRole('option', { name: 'rate' })).toHaveAttribute('aria-selected', 'true');
		await page.keyboard.press('Enter');
		await expect(pick).toHaveCount(0);
		await expect.poll(async () => (await nodeParams(page, osc!)).lfo.frequency.expression).toBe("sm('beat').rate");
		expect(await placed()).toEqual(before);

		// In the row's editor, under its label, the completion window opens with the focus, as an
		// extension of the field: its width, its top on the field's bottom edge. Typed at a hand's
		// pace, every keystroke leaves the list catching up, and Tab lands while it still is: Tab
		// takes the option the list settles on, and the window stays. Escape alone closes it,
		// Ctrl+Space brings it back, and Enter commits the line.
		await row.getByRole('button', { name: 'frequency', exact: true }).click();
		const editor = row.getByTestId('param-expr-input');
		await expect(editor).toBeVisible();
		await editor.click();
		const win = page.getByTestId('expr-window');
		await expect(win).toBeVisible();
		const field = (await row.locator('.expr-host').boundingBox())!;
		const box = (await win.boundingBox())!;
		expect([box.x, box.width, box.y].map(Math.round)).toEqual([field.x, field.width, field.y + field.height].map(Math.round));
		await page.keyboard.press('Control+a');
		await page.keyboard.type('vars.beat.ga', { delay: 40 });
		await page.keyboard.press('Tab');
		await expect(editor).toHaveText('vars.beat.gain');
		await expect(win).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(win).toBeHidden();
		await expect(editor).toBeFocused();
		await page.keyboard.press('Control+Space');
		await expect(win).toBeVisible();
		await page.keyboard.press('Enter');
		await expect.poll(async () => (await nodeParams(page, osc!)).lfo.frequency.expression).toBe('vars.beat.gain');

		// Delete goes to the active panel alone: with the box selected in the machine panel it takes
		// the box and leaves the node selected in the editor; with the editor active, the reverse.
		// The playhead moves first: a state it starts in refuses to go.
		await rawCall(page, 'machine state add', { machine: 'machine0', name: 'state1' });
		await rawCall(page, 'machine playhead edit', { machine: 'machine0', name: 'beat', start: 'state1' });
		const card1 = page.getByTestId('state-card-state1');
		await expect(card1).toBeVisible();
		await name.click();
		await expect(page.getByTestId('machine-inspector').getByTestId('column-state0')).toHaveAttribute('data-selected', 'true');
		await page.keyboard.press('Delete');
		await expect(card).toHaveCount(0);
		expect((await nodes(page)).some((n) => n.uid === osc)).toBe(true);
		await card1.getByTestId('state-card-name').click();
		await selectNode(page, osc);
		await page.keyboard.press('Delete');
		await waitForNoNode(page, osc);
		osc = null;
		await expect(card1).toBeVisible();
		await expectIntact(page, 'the machine panel beside the editor');
	} finally {
		await closeSplit(page);
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});
