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
async function drag(page: Page, hasTouch: boolean, from: Locator, to: Locator): Promise<string | null> {
	const [a, b] = await Promise.all([from.boundingBox(), to.boundingBox()]);
	const start = { x: a!.x + a!.width - 3, y: a!.y + a!.height / 2 };
	const end = { x: b!.x + b!.width * 0.25, y: b!.y + b!.height * 0.4 };
	if (hasTouch) { await swipe(page, start, end); return null; }
	await page.mouse.move(start.x, start.y);
	await page.mouse.down();
	await page.mouse.move(end.x, end.y, { steps: 8 });
	const preview = page.locator('.svelte-flow__connection .svelte-flow__edge-path');
	await expect(preview).toBeVisible();
	const path = await preview.getAttribute('d');
	await page.mouse.up();
	return path;
}

test('a machine is built by gesture and its playhead crosses on a tap', async ({ page, hasTouch }, testInfo) => {
	const browserErrors: string[] = [];
	page.on('pageerror', error => browserErrors.push(error.message));
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
		const clearSelection = async (): Promise<void> => {
			const bounds = (await pane.boundingBox())!;
			const position = { x: bounds.width * 0.55, y: bounds.height * 0.04 };
			if (hasTouch) await pane.tap({ position });
			else await pane.click({ position });
			await expect(page.getByTestId('machine-inspector')).toHaveAttribute('data-subject', 'machine');
		};
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
		const preview = await drag(page, hasTouch, card0, card1);
		const label = page.getByTestId('transition-t1');
		await expect(label).toHaveText('manual');
		if (preview) await expect(page.locator('.svelte-flow__edge[data-id="t1"] .svelte-flow__edge-path')).toHaveAttribute('d', preview);
		const inspector = page.getByTestId('machine-inspector');
		await expect(inspector).toHaveAttribute('data-subject', 'transition');
		await expect(page.getByTestId('machine-transition')).toBeVisible();

		// A box selected shows its name and the transition out of it; the bare canvas shows the machine.
		await press(card0);
		await expect(inspector).toHaveAttribute('data-subject', 'state');
		await expect(inspector.getByTestId('state-name')).toHaveText('state0');
		await press(inspector.getByTestId('state-name'));
		await inspector.getByTestId('state-name-input').fill('cancelled');
		await inspector.getByTestId('state-name-input').press('Escape');
		await expect(inspector).toHaveAttribute('data-subject', 'state');
		await expect(inspector.getByTestId('state-name')).toHaveText('state0');
		await expect(inspector.getByTestId('transitions-out').getByTestId('transition-row-t1')).toBeVisible();
		await expect(inspector.getByTestId('transitions-in').getByTestId('transition-row-t1')).toHaveCount(0);
		await press(card1);
		await expect(inspector.getByTestId('state-name')).toHaveText('state1');
		await expect(inspector.getByTestId('transitions-out').getByTestId('transition-row-t1')).toHaveCount(0);
		await expect(inspector.getByTestId('transitions-in').getByTestId('transition-row-t1')).toBeVisible();
		await press(card0);
		await press(inspector.getByTestId('transitions-out').getByTestId('transition-row-t1'));
		await expect(inspector).toHaveAttribute('data-subject', 'transition');
		await expect(page.getByTestId('machine-transition')).toHaveCount(1);
		await press(card0);
		await expect(inspector).toHaveAttribute('data-subject', 'state');
		await clearSelection();

		// A playhead is born in the first state, and its dot sits on that box's top edge.
		await press(page.getByTestId('machine-add-playhead'));
		const dot = page.getByTestId('playhead-dot');
		await expect(dot).toHaveAttribute('data-state', 'state0');
		await expect.poll(() => onTopEdge(dot, card0)).toBe(true);
		await expect(page.getByTestId('playhead-state')).toHaveText('state0');

		// A tap on the label selects it; the inspector's fire button moves the playhead, and the dot
		// comes to rest on the target box.
		await press(label);
		await expect(inspector).toHaveAttribute('data-subject', 'transition');
		await press(inspector.getByTestId('transition-fire'));
		await expect(dot).toHaveAttribute('data-state', 'state1');
		await expect(dot).toHaveAttribute('data-flying', 'false');
		await expect.poll(() => onTopEdge(dot, card1)).toBe(true);
		await clearSelection();
		await expect(page.getByTestId('playhead-state')).toHaveText('state1');

		// The machine's name and seed are the inspector's; the bar's select is the switch between machines.
		await expect(inspector.getByTestId('machine-name')).toHaveText('machine0');
		await expect(inspector.getByTestId('machine-seed')).toHaveValue('0');
		await expect(panel.getByTestId('machine-switch').locator('select')).toHaveValue('machine0');

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
		// either label selects its own.
		await rawCall(page, 'machine transition add', { machine: 'machine0', from: 'state1', to: 'state0', triggers: [{ kind: 'manual' }] });
		const back = page.getByTestId('transition-t2');
		await expect(back).toBeVisible();
		const [l1, l2] = await Promise.all([label.boundingBox(), back.boundingBox()]);
		expect(l1!.x + l1!.width <= l2!.x || l2!.x + l2!.width <= l1!.x || l1!.y + l1!.height <= l2!.y || l2!.y + l2!.height <= l1!.y, `the two labels do not overlap: ${JSON.stringify(l1)} ${JSON.stringify(l2)}`).toBe(true);
		await press(back);
		await expect(page.locator('.svelte-flow__edge.selected')).toHaveAttribute('data-id', /t2/);
		if (!hasTouch) {
			await label.click({ modifiers: ['Shift'] });
			await expect(page.locator('.svelte-flow__edge.selected')).toHaveCount(2);
			await label.click({ modifiers: ['Shift'] });
			await expect(page.locator('.svelte-flow__edge.selected')).toHaveAttribute('data-id', /t2/);
		}
		await page.keyboard.press('Delete');
		await expect(back).toHaveCount(0);

		// Selection policy and priority are authored through the state inspector.
		await rawCall(page, 'machine transition add', { machine: 'machine0', from: 'state0', to: 'state1', triggers: [{ kind: 'manual' }] });
		await press(card0);
		const selection = inspector.getByRole('combobox', { name: 'transition selection' });
		await expect(selection).toHaveValue('ordered');
		await selection.selectOption('weighted');
		await expect.poll(async () => (await rawCall(page, 'machine list')).result.machines.machine0.states.state0.selection).toBe('weighted');
		await press(inspector.getByTestId('transition-up-t2'));
		await expect.poll(async () => (await rawCall(page, 'machine list')).result.outgoing.machine0.state0).toEqual(['t2', 't1']);
		await selection.selectOption('uniform');
		await expect(selection).toHaveValue('uniform');
		await rawCall(page, 'machine transition remove', { machine: 'machine0', id: 't2' });

		// One duration control exposes fixed, expression and seeded random range modes. Endpoints
		// remain editable; the wildcard source draws a route from each real source card.
		await press(label);
		await press(inspector.getByRole('button', { name: 'random range', exact: true }));
		const minimum = inspector.getByRole('textbox', { name: 'duration minimum' });
		const maximum = inspector.getByRole('textbox', { name: 'duration maximum' });
		await minimum.fill('0.25');
		await minimum.press('Enter');
		await maximum.fill('0.5');
		await maximum.press('Enter');
		await expect.poll(async () => (await rawCall(page, 'machine list')).result.machines.machine0.transitions.t1.duration).toEqual({ min: 0.25, max: 0.5 });
		await press(inspector.getByRole('button', { name: 'fixed', exact: true }));
		await inspector.getByRole('combobox', { name: 'from state', exact: true }).selectOption('*');
		await expect(page.locator('.svelte-flow__edge[data-id="t1@state0"]')).toBeVisible();
		await expect(page.locator('.svelte-flow__edge[data-id="t1@state1"]')).toBeVisible();
		await inspector.getByRole('combobox', { name: 'from state', exact: true }).selectOption('state0');
		await expect(label).toHaveText('manual');

		// The first goes with the Delete key too, picked from its label; the box's ✕ is gone with it.
		await press(label);
		await expect(inspector).toHaveAttribute('data-subject', 'transition');
		await page.keyboard.press('Delete');
		await expect(label).toHaveCount(0);
		await expect(card0.getByTestId('state-remove')).toHaveCount(0);

		// The playhead's group is the machine's: listed locked whole, like a device's.
		const groups = (await rawCall(page, 'session state')).result.variable_groups;
		expect(groups.playhead0.machine).toBe('machine0');
		const viewport = panel.locator('.svelte-flow__viewport');
		const framing = await viewport.getAttribute('style');
		for (const name of ['renamed', 'machine0']) {
			await press(inspector.getByTestId('machine-name'));
			await inspector.getByTestId('machine-name-input').fill(name);
			await inspector.getByTestId('machine-name-input').press('Enter');
			await expect(panel).toHaveAttribute('data-machine', name);
			await expect(viewport).toHaveAttribute('style', framing!);
		}

		// Close cards, long parallel labels, and more residents than the default dock can hold.
		await rawCall(page, 'machine jump', { machine: 'machine0', playhead: 'playhead0', state: 'state0' });
		for (let i = 1; i < 14; i++) await rawCall(page, 'machine playhead add', { machine: 'machine0', name: `crowd${i}`, start: 'state0' });
		for (let i = 0; i < 3; i++) await rawCall(page, 'machine transition add', {
			machine: 'machine0', from: 'state0', to: 'state1', triggers: [{ kind: 'event', name: `a_long_event_label_${i}_with_details` }]
		});
		await rawCall(page, 'machine state add', { machine: 'machine0', name: 'fanout', pos: [600, 50] });
		await rawCall(page, 'machine transition add', { machine: 'machine0', from: 'state0', to: 'fanout',
			triggers: [{ kind: 'event', name: 'another_long_label_from_a_different_pair' }] });
		const self = (await rawCall(page, 'machine transition add', { machine: 'machine0', from: 'state0', to: 'state0',
			duration: 1, triggers: [{ kind: 'manual' }] })).result.id;
		for (const pos of [[220, 0], [0, 60]]) {
			await rawCall(page, 'machine state edit', { machine: 'machine0', name: 'state0', pos: [0, 0] });
			await rawCall(page, 'machine state edit', { machine: 'machine0', name: 'state1', pos });
			await expect.poll(async () => {
				const labels = await page.getByTestId(/^transition-t/).all();
				const cards = await Promise.all([card0.boundingBox(), card1.boundingBox(), page.getByTestId('state-card-fanout').boundingBox()]);
				const boxes = await Promise.all(labels.map((l) => l.boundingBox()));
				const overlaps = (a: NonNullable<typeof boxes[number]>, b: NonNullable<typeof boxes[number]>): boolean =>
					a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
				return boxes.every((a, i) => a && [...cards, ...boxes.slice(i + 1)].every((b) => b && !overlaps(a, b)));
			}).toBe(true);
		}
		await expect.poll(async () => {
			const label = await page.getByTestId(`transition-${self}`).boundingBox();
			const midpoint = await page.locator(`.svelte-flow__edge[data-id="${self}"] .svelte-flow__edge-path`).evaluate((el) => {
				const path = el as SVGPathElement;
				const main = document.createElementNS('http://www.w3.org/2000/svg', 'path');
				main.setAttribute('d', path.getAttribute('d')!.split(' M ')[0]);
				const p = main.getPointAtLength(main.getTotalLength() / 2).matrixTransform(path.getScreenCTM()!);
				return { x: p.x, y: p.y };
			});
			return label && Math.hypot(label.x + label.width / 2 - midpoint.x, label.y + label.height / 2 - midpoint.y) <= 1.5;
		}).toBe(true);
		await expect(page.getByTestId('playhead-dot')).toHaveCount(14);
		for (const resident of await page.getByTestId('playhead-dot').all()) await expect.poll(() => onTopEdge(resident, card0)).toBe(true);

		// Runtime health reaches the inspector through the socket replica and clears on recovery.
		const fault = (await rawCall(page, 'machine transition add', { machine: 'machine0', from: 'state0', to: 'state0',
			triggers: [{ kind: 'always' }] })).result.id;
		const health = inspector.getByTestId('machine-health');
		await expect(health).toContainText('zero-time transition cycle');
		await rawCall(page, 'machine state edit', { machine: 'machine0', name: 'state0', pos: [10, 10] });
		await expect(health).toContainText('zero-time transition cycle');
		await rawCall(page, 'machine transition edit', { machine: 'machine0', id: fault, triggers: [{ kind: 'manual' }] });
		await expect(health).toHaveCount(0);
		await rawCall(page, 'machine transition edit', { machine: 'machine0', id: fault,
			triggers: [{ kind: 'when', expression: 'variables.health_missing.gate', edge: 'level' }] });
		await expect(health).toContainText('health_missing.gate');
		await rawCall(page, 'variable entry add', { name: 'health_missing.gate', value: 0 });
		await expect(health).toHaveCount(0);

		expect(browserErrors, 'machine gestures and previews have no runtime errors').toEqual([]);
		await expectIntact(page, 'the machine panel');
	} finally {
		await restorePanelType(page);
		await rawCall(page, 'session new');
	}
});
