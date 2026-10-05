// One patch, built with a finger.
//
// AGENTS.md makes it a hard constraint that no interaction lives solely behind hover, right-click
// or a keyboard chord, and that touch gets its own door "gated on the one spelling a test
// enforces". This is that test. It is a SESSION rather than a suite because a gesture only means
// something in sequence: a tap after a long press, a drag after a tap, a second tap that is either
// a double or a new gesture depending on what came before.
//
// What is NOT here: tap-target sizes, coarse font sizes, and everything else about how the app
// MEASURES under a finger. Those are invariants now, swept over the whole scene by
// `integrity.spec.ts` under this same Pixel 7 project — a rule that names no element beats fifty
// tests that each name one.

import { test, expect, type Locator, type Page } from '@playwright/test';
import { restorePanelType, waitForApp } from '../lib/app';
import { addNode, frameSummary, tapNode, waitForNode } from '../lib/goofi';
import { rawCall } from '../lib/harness';
import { emptySpot, pinch, swipe, touchSession } from '../lib/touch';
import { pane } from '../lib/inspector';

/** A long press at `p`, the coarse door onto everything hover and right-click own on a desktop:
 * the finger stays down until `opened` shows. */
async function longPress(page: Page, p: { x: number; y: number }, opened: Locator): Promise<void> {
	const touch = await touchSession(page);
	await touch.down(p);
	await expect(opened, 'the held finger opened its menu').toBeVisible();
	await touch.up();
}

test('a held parameter opens modulation without toggling its disclosure', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	const uid = await addNode(page, 'LFO');
	try {
		await waitForNode(page, uid);
		await tapNode(page, uid);
		const field = pane(page).getByTestId('param-field-frequency');
		await field.scrollIntoViewIfNeeded();
		const summary = field.getByRole('button', { name: 'frequency', exact: true });
		const box = await field.boundingBox();
		const choice = page.getByRole('menuitem', { name: 'LFO', exact: true });
		await longPress(page, { x: box!.x + 2, y: box!.y + 2 }, choice);
		await expect(summary).toHaveAttribute('aria-expanded', 'false');
		await expect(choice).toBeVisible();
		await choice.tap();
		await expect.poll(() => page.evaluate((id) =>
			(window as any).goofi.query.graph().nodes.find((n: { uid: string }) => n.uid === id)?.params.lfo.frequency.mode, uid
		)).toBe('expression');
	} finally {
		await tearDown(page);
	}
});

test('a pinch past legibility lets a viewer\'s stream go, and a pinch back resumes it', async ({ page }) => {
	// `ViewerFeed.svelte` unsubscribes below a 0.3 zoom and keeps the last frame; a pinch is how a
	// phone gets there. The proof is on the wire: `/data` stops arriving for the slot, then arrives
	// again. The fingers close ABOUT the card, so it shrinks in place and never leaves the viewport,
	// which would let the stream go for a reason that is not the zoom.
	await page.goto('/');
	await waitForApp(page);
	try {
		const osc = await addNode(page, 'LFO', [40, 200]);
		await waitForNode(page, osc);
		await page.evaluate((u) => {
			const g = (window as any).goofi;
			g.commands.updateParam(u, 'output', 'mode', 'block');
			g.commands.updateParam(u, 'output', 'sfreq', 64);
		}, osc);
		const card = page.locator(`.svelte-flow__node[data-id="${osc}"]`);
		await expect(card.locator('.slot-viewer .body')).toBeVisible();
		await expect.poll(() => frameSummary(page, osc)).not.toBeNull();
		const rate = () =>
			page.evaluate((u) => (window as any).goofi.query.arrivalRate(u, 'out') ?? 0, osc);
		await expect.poll(rate, 'the viewer draws the stream').toBeGreaterThan(0);

		// The pane's own scale, read off its transform: the number the freeze is decided from.
		const zoom = () =>
			page.locator('.svelte-flow__viewport').evaluate(
				(el) => Number(/scale\(([\d.]+)\)/.exec(el.style.transform)?.[1] ?? 1)
			);
		const pinchTo = async (reached: (z: number) => boolean, factor: number) => {
			await expect
				.poll(async () => {
					if (reached(await zoom())) return true;
					const c = (await card.boundingBox())!;
					const centre = { x: Math.round(c.x + c.width / 2), y: Math.round(c.y + c.height / 2) };
					// Fingers clear of the card on either side, or the pinch drags the card instead.
					const gap = Math.round(c.height + 2 * 60);
					await pinch(page, centre, gap, gap * factor);
					return reached(await zoom());
				})
				.toBe(true);
		};
		await pinchTo((z) => z < 0.28, 0.5);
		await expect.poll(rate, 'past the threshold, nothing arrives for the slot').toBe(0);
		await expect.poll(() => frameSummary(page, osc)).toBeNull();
		await pinchTo((z) => z > 0.4, 2);
		await expect.poll(rate, 'and back above it, the stream is demanded again').toBeGreaterThan(0);
	} finally {
		await tearDown(page);
	}
});

async function tearDown(page: Page): Promise<void> {
	await page.evaluate(async () => {
		const g = (window as any).goofi;
		const uids = g.query.graph().nodes.map((n: { uid: string }) => n.uid);
		if (uids.length) await g.commands.removeNodes(uids);
	});
	await expect
		.poll(() => page.evaluate(() => (window as any).goofi.query.graph().nodes.length))
		.toBe(0);
}

test('a patch authored with a finger, and every door hover owns on a desktop', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		await test.step('a long press on bare canvas opens the add-node menu, where the finger was', async () => {
			// The coarse door onto right-click. Without it the canvas has no way in at all on a phone.
			const spot = await emptySpot(page);
			const menu = page.getByTestId('add-node-menu-anchor');
			const touch = await touchSession(page);
			await touch.down(spot);
			await expect(menu, 'the press opens it while the finger is still down').toBeVisible();
			// The touchend ENDING the opening gesture must not read as a dismissal: it lands on the
			// click-catcher the press itself mounted mid-gesture.
			await touch.up();
			await expect(menu, 'and the release does not close what the press opened').toBeVisible();
			const box = (await menu.boundingBox())!;
			const view = page.viewportSize()!;
			// Anchored to the finger, stated as a property rather than a distance: the press point
			// lies within one touch target of the menu it opened, wherever the clamp had to put it.
			// A pinned number here would have to move every time the menu's own height does, and
			// containment alone was one: the menu clears the finger by design, so it holds only
			// while the menu is tall enough for the clamp to push it back up.
			const hit = await page.evaluate(() =>
				parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--hit'))
			);
			expect(spot.y, 'the finger is at what the press opened').toBeGreaterThanOrEqual(box.y - hit);
			expect(spot.y).toBeLessThanOrEqual(box.y + box.height + hit);
			expect(box.x, 'and the clamp keeps it wholly on screen').toBeGreaterThanOrEqual(0);
			expect(box.x + box.width).toBeLessThanOrEqual(view.width);
			expect(box.y).toBeGreaterThanOrEqual(0);
			expect(box.y + box.height).toBeLessThanOrEqual(view.height + 1);
			await page.keyboard.press('Escape');
			await expect(menu).toHaveCount(0);
		});

		let osc = '';
		let lfo = '';
		let lfoName = '';
		await test.step('a tap selects a node, and the inspector arrives as a sheet', async () => {
			osc = await addNode(page, 'LFO', [40, 40]);
			await waitForNode(page, osc);
			await tapNode(page, osc);
			await expect
				.poll(() => page.evaluate(() => (window as any).goofi.query.selection().nodes.length))
				.toBe(1);
			await expect(pane(page), 'a single selection opens the inspector').toHaveClass(/open/);
		});

		await test.step('a source is chosen by finger: the fx chip seeds an expression, and = retains it', async () => {
			lfo = await addNode(page, 'LFO', [40, 260]);
			await waitForNode(page, lfo);
			const nameOf = (u: string): Promise<string> =>
				page.evaluate(
					(uid) => (window as any).goofi.query.graph().nodes.find((n: { uid: string }) => n.uid === uid)?.name,
					u
				);
			lfoName = await nameOf(lfo);
			await tapNode(page, osc);
			await expect(pane(page)).toHaveClass(/open/);
			const field = pane(page).getByTestId('param-field-frequency');
			// The entry background opens the source controls.
			await field.tap({ position: { x: 2, y: 2 } });
			await expect(field.getByTestId('param-more'), 'the background opened the source row').toBeVisible();
			const frequency = (key: 'mode' | 'expression'): Promise<unknown> =>
				page.evaluate(
					([u, k]) =>
						(window as any).goofi.query.graph().nodes.find((n: { uid: string }) => n.uid === u)?.params
							.lfo.frequency[k],
					[osc, key] as const
				);
			await field.getByTestId('param-mode-expression').tap();
			await expect.poll(() => frequency('mode')).toBe('expression');
			await expect
				.poll(async () => typeof (await frequency('expression')) === 'string', {
					message: 'switching to fx seeded an expression from the literal'
				})
				.toBe(true);
			const seeded = await frequency('expression');
			// `=` by the same finger returns to the constant with the expression RETAINED.
			await field.getByTestId('param-mode-constant').tap();
			await expect.poll(() => frequency('mode')).toBe('constant');
			expect.soft(await frequency('expression'), 'a mode switch retains the expression').toBe(seeded);
		});

		await test.step('a node carried onto a param row is read by it', async () => {
			const row = pane(page).getByTestId('param-field-amplitude');
			const card = (await page.locator(`.svelte-flow__node[data-id="${lfo}"] .header`).boundingBox())!;
			const at = (await row.boundingBox())!;
			const to = { x: Math.round(at.x + at.width / 2), y: Math.round(at.y + at.height / 2) };
			await swipe(page, { x: Math.round(card.x + card.width / 2), y: Math.round(card.y + card.height / 2) }, to);
			await expect
				.poll(() =>
					page.evaluate(
						(u) =>
							(window as any).goofi.query.graph().nodes.find((n: { uid: string }) => n.uid === u)?.params
								.lfo.amplitude.expression,
						osc
					)
				)
				.toBe(`nd('${lfoName}')`);
		});

		await test.step('a long press on a control tells what it does, and does NOT do it', async () => {
			// The other half of the hover door: a desktop learns a control's name by hovering it, and a
			// finger has to be able to ask without also pressing. Named by test id rather than by
			// ordinal — the header's right end is a progressive overflow, so which control is first
			// there is a function of the panel's width.
			const tip = page.getByTestId('title-tip');
			const btn = page.getByTestId('panel-maximize').first();
			const b = (await btn.boundingBox())!;
			const touch = await touchSession(page);
			await touch.down({ x: Math.round(b.x + b.width / 2), y: Math.round(b.y + b.height / 2) });
			await expect(tip, 'the press surfaces the title the finger is on').toHaveText('Maximize');
			await touch.up();
			// `toggleMaximize` would have flipped this to "Restore" had the press gone through as a
			// click. Asking what a control does must never be the same as using it.
			await expect(tip, 'and releasing leaves the answer standing').toHaveText('Maximize');
		});

		await test.step('…while a TAP acts, and raises no tip', async () => {
			const tabs = page.getByTestId('param-tabs');
			const other = tabs.getByRole('tab', { selected: false }).first();
			const name = (await other.textContent())!.trim();
			await other.tap();
			await expect(tabs.getByRole('tab', { selected: true })).toHaveText(name);
			await expect(page.getByTestId('title-tip'), 'a tap is not a press').toHaveCount(0);
		});

		await test.step('the sheet resizes by dragging its own seam, and a finger cannot throw it away', async () => {
			// The one gesture the pane has, identical to the mouse's. Dragging it far past its floor
			// used to dismiss the pane, which made the only way out of the inspector a swipe nobody
			// documented; the ✕ is the whole of it now.
			const grip = page.getByTestId('panel-resize-handle').first();
			const g = (await grip.boundingBox())!;
            const from = { x: Math.round(g.x + g.width / 2), y: Math.round(g.y + g.height / 2) };
			const before = (await pane(page).boundingBox())!;
			await swipe(page, from, { x: from.x, y: from.y + 400 });
			await expect(pane(page), 'still open — only the ✕ closes it').toHaveClass(/open/);
			const after = (await pane(page).boundingBox())!;
			expect(after.height, 'and the drag resized it').toBeLessThan(before.height);
		});

		await test.step('the ✕ is the way out, and the next tap brings the pane back', async () => {
			await pane(page).getByTestId('inspector-close').tap();
			await expect(pane(page)).not.toHaveClass(/open/);
			// A NEW selection, not the same one again: the ✕ dismisses the pane and leaves the node
			// selected, so re-tapping it changes nothing. That is what "not an off-switch" means — the
			// dismissal lasts exactly until the next selection.
			await page.evaluate(() => (window as any).goofi.commands.clearSelection());
			await expect
				.poll(() => page.evaluate(() => (window as any).goofi.query.selection().nodes.length))
				.toBe(0);
			await tapNode(page, osc);
			await expect(pane(page), 'a dismiss is not an off-switch').toHaveClass(/open/);
		});
		await test.step('a drag on the canvas PANS, and opens nothing', async () => {
			// LAST, because it moves the viewport: every step above has to be able to reach the node it
			// put there. A pan must not read as the press that opens the menu, or the canvas becomes
			// unusable the moment anyone scrolls it.
			const from = await emptySpot(page);
			await swipe(page, from, { x: from.x - 90, y: from.y - 60 });
			await expect(
				page.getByTestId('add-node-menu-anchor'),
				'a pan is not a press'
			).toHaveCount(0);
		});
		await test.step('a control widget turns under a finger, and MOVES once the panel is in edit mode', async () => {
			// LAST: it turns the editor panel into a control panel, so every canvas step above
			// still has its canvas.
			await page.evaluate(async () => {
				const g = (window as any).goofi;
				await g.commands.addVariable('desk.level', 0.5, {
					kind: 'knob', min: 0, max: 1, step: 0.01, x: 0, y: 0, w: 3, h: 3
				});
				const panel = g.query.panels()[0];
				g.commands.setPanelType(panel.panelId, 'control');
				g.commands.setPanelState(panel.panelId, { group: 'desk' });
			});
			await expect(page.getByTestId('control-panel')).toHaveAttribute('data-edit', 'true');
			await page.getByTestId('control-edit-toggle').tap();
			const knob = page.getByTestId('control-desk-level');
			await expect(knob).toBeVisible();
			const level = () =>
				page.evaluate(
					() => (window as any).goofi.query.variables().find((g: { name: string }) => g.name === 'desk.level').value
				);

			// The claim is the GESTURE, so the finger does it: a drag up turns the knob.
			const box = (await knob.boundingBox())!;
			const centre = { x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2) };
			await swipe(page, centre, { x: centre.x, y: centre.y - 60 });
			await expect.poll(level, 'a drag on a widget turns it').toBeGreaterThan(0.5);

			// …and the same drag MOVES it once edit mode is on, which is the whole mode switch.
			await page.getByTestId('control-edit-toggle').tap();
			// The strip opening above the board is the sign the mode landed, and it moves the board.
			await expect(page.getByTestId('control-palette'), 'edit mode opened the palette').toBeVisible();
			const before = (await knob.boundingBox())!;
			const held = await level();
			const grip = { x: Math.round(before.x + 6), y: Math.round(before.y + 6) };
			await swipe(page, grip, { x: grip.x + Math.round(before.width), y: grip.y });
			await expect
				.poll(async () => (await knob.boundingBox())!.x, 'the widget moved')
				.toBeGreaterThan(before.x);
			expect.soft(await level(), 'and moving it did not turn it').toBe(held);

			// Edit mode also opens the palette, and a chip dragged onto the board bears a widget where
			// it lands, named for its kind — the one door a new element has.
			// …let go BELOW the board's own box, in the empty scroll area: the board is only as tall as
			// its widgets, and a drop on the space under them is the drop a finger makes.
			const chip = page.getByTestId('control-palette-slider');
			const cb = (await chip.boundingBox())!;
			const bb = (await page.getByTestId('control-board').boundingBox())!;
			await swipe(
				page,
				{ x: Math.round(cb.x + cb.width / 2), y: Math.round(cb.y + cb.height / 2) },
				{ x: Math.round(bb.x + bb.width * 0.3), y: Math.round(bb.y + bb.height + 40) }
			);
			await expect(
				page.getByTestId('control-desk-slider0'),
				'the drop bore a slider with a fresh name'
			).toBeVisible();

			// The grab picked the knob, so its form is open; a finger's learn is there, and a machine
			// with no MIDI device is told so rather than left listening to nothing.
			const learn = page.getByTestId('control-props').getByTestId('control-learn');
			await learn.tap();
			if (((await rawCall(page, 'midi list', {})).result.ports as unknown[]).length === 0) {
				await expect(page.getByTestId('toast'), 'learn with no MIDI device says so').toContainText('MIDI');
			} else {
				await expect(learn).toHaveAttribute('aria-pressed', 'true');
				await learn.tap();
			}
		});
	} finally {
		await restorePanelType(page);
		await page.evaluate(async () => {
			const g = (window as any).goofi;
			for (const v of g.query.variables().filter((v: { name: string }) => v.name.startsWith('desk.')))
				await g.commands.removeVariable(v.name);
		});
		await tearDown(page);
	}
});

test('a viewer panel\'s strip is one group that slides as a whole, under a short floor', async ({ page }) => {
	// The dot, the node picker, the slot, the kind and the cog scroll together when the panel is
	// narrower than they are; none stays pinned while the rest clip. Their floor is shorter than
	// the 44px hit, so the phone keeps its height for the data.
	await page.goto('/');
	await waitForApp(page);
	try {
		const c = await addNode(page, 'signal:Constant', [40, 200]);
		await waitForNode(page, c);
		await page.evaluate((u) => {
			const g = (window as any).goofi;
			const p = g.query.panels()[0].panelId;
			g.commands.setPanelType(p, 'viewer');
			g.commands.bindNodeToPanel(p, u);
		}, c);
		const strip = page.getByTestId('panel-strip');
		await expect(strip.getByTestId('panel-node')).toBeVisible();
		await expect(strip.getByTestId('viewer-slot')).toBeAttached();
		await expect(strip.getByTestId('viewer-kind')).toBeAttached();
		await expect(strip.getByTestId('viewer-settings-cog')).toBeAttached();
		const height = (await strip.getByTestId('viewer-slot').locator('select').boundingBox())!.height;
		expect(height, 'the slot picker reads as the node header\'s does').toBeLessThanOrEqual(28);
	} finally {
		await restorePanelType(page);
		await tearDown(page);
	}
});

test('a finger held on a viewer reads it, and follows as it moves, without moving the card', async ({ page }) => {
	// There is no hover on a touch screen, so the finger IS the hover: the readout shows while it
	// is down and follows it. The finger is reading the picture, not carrying the node. An image
	// answers under every cell, so the readout's presence is the finger's alone.
	await page.goto('/');
	await waitForApp(page);
	try {
		const c = await addNode(page, 'signal:Constant', [40, 200]);
		await waitForNode(page, c);
		await page.evaluate((u) => (window as any).goofi.commands.updateParam(u, 'constant', 'shape', '4,4'), c);
		await rawCall(page, 'node edit', { node: c, viewer: [{ slot: 'out', kind: 'image' }] });
		const card = page.locator(`.svelte-flow__node[data-id="${c}"]`);
		const body = card.locator('.slot-viewer .body');
		await expect(body).toBeVisible();
		await expect.poll(() => frameSummary(page, c)).not.toBeNull();
		const before = (await card.boundingBox())!;
		const box = (await body.boundingBox())!;
		const readout = page.locator('.viewer-hover-readout');
		const touch = await touchSession(page);
		const at = { x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2) };
		await touch.down(at);
		await expect(readout, 'the press reads the cell under the finger').toHaveCount(1);
		await touch.moveTo({ x: at.x + 30, y: at.y + 10 });
		await expect(readout, 'and holds through the move').toHaveCount(1);
		await page.waitForTimeout(150);
		await touch.up();
		await expect(readout, 'and goes with the finger').toHaveCount(0);
		const after = (await card.boundingBox())!;
		expect([after.x, after.y], 'the card stayed where it was').toEqual([before.x, before.y]);
	} finally {
		await tearDown(page);
	}
});

test('a held slot or control element is picked for reference, and a held param takes it', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	const param = (u: string, name: string, key: 'mode' | 'expression'): Promise<unknown> =>
		page.evaluate(
			([uid, n, k]) =>
				(window as any).goofi.query.graph().nodes.find((x: { uid: string }) => x.uid === uid)?.params.lfo[n][k],
			[u, name, key] as const
		);
	const centre = async (l: Locator) => {
		const b = (await l.boundingBox())!;
		return { x: Math.round(b.x + b.width / 2), y: Math.round(b.y + b.height / 2) };
	};
	const corner = async (l: Locator) => {
		const b = (await l.boundingBox())!;
		return { x: Math.round(b.x + 2), y: Math.round(b.y + 2) };
	};
	try {
		const osc = await addNode(page, 'LFO', [40, 40]);
		const lfo = await addNode(page, 'LFO', [40, 260]);
		await waitForNode(page, osc);
		await waitForNode(page, lfo);

		await test.step('a double tap on the inspector\'s name field renames the node', async () => {
			await tapNode(page, lfo);
			const name = pane(page).getByTestId('node-name');
			// A trial waits for the sheet to come to rest under the name.
			await name.tap({ trial: true });
			const at = await centre(name);
			const touch = await touchSession(page);
			for (let i = 0; i < 2; i++) {
				await touch.down(at);
				await touch.up();
			}
			const input = pane(page).getByTestId('node-name-input');
			await expect(input, 'the taps opened the field and kept it').toBeFocused();
			await input.fill('carrier');
			await input.press('Enter');
			await expect
				.poll(() => page.evaluate((u) => (window as any).goofi.query.graph().nodes.find((n: { uid: string }) => n.uid === u)?.name, lfo))
				.toBe('carrier');
		});

		await test.step('a held output slot is picked, and a held param reads it', async () => {
			const pin = page.locator(`.svelte-flow__node[data-id="${lfo}"] [data-testid="slot-output-pin"]`);
			const pick = page.getByRole('menuitem', { name: 'Select for reference' });
			// The sheet covers the canvas; its ✕ clears the way to the slot.
			await pane(page).getByTestId('inspector-close').tap();
			await expect(pane(page)).not.toHaveClass(/open/);
			await longPress(page, await centre(pin), pick);
			await pick.tap();
			await tapNode(page, osc);
			const field = pane(page).getByTestId('param-field-frequency');
			await field.scrollIntoViewIfNeeded();
			const take = page.getByRole('menuitem', { name: "Reference selection: nd('carrier')" });
			await longPress(page, await corner(field), take);
			await take.tap();
			await expect.poll(() => param(osc, 'frequency', 'mode')).toBe('expression');
			expect(await param(osc, 'frequency', 'expression')).toBe("nd('carrier')");
			await expect(page.getByTestId('reference-edge'), 'the selected node draws what it reads').toHaveCount(1);
		});

		await test.step('a held control element is picked, and a held param reads it', async () => {
			await page.evaluate(async () => {
				const g = (window as any).goofi;
				await g.commands.addVariable('desk.level', 0.5, {
					kind: 'knob', min: 0, max: 1, step: 0.01, x: 0, y: 0, w: 3, h: 3
				});
				const panel = g.query.panels()[0];
				g.commands.setPanelType(panel.panelId, 'control');
				g.commands.setPanelState(panel.panelId, { group: 'desk' });
			});
			await expect(page.getByTestId('control-panel')).toHaveAttribute('data-edit', 'true');
			await page.getByTestId('control-edit-toggle').tap();
			await expect(page.getByTestId('control-panel')).toHaveAttribute('data-edit', 'false');
			const label = page.getByTestId('control-desk-level').locator('.label');
			await label.dblclick();
			const rename = page.getByTestId('control-rename');
			await rename.fill('other');
			await rename.press('Escape');
			await expect(rename, 'Escape closes the rename field').toHaveCount(0);
			await expect(label, 'and keeps the old name').toHaveText('level');
			const pick = page.getByRole('menuitem', { name: 'Select for reference' });
			await longPress(page, await centre(label), pick);
			await pick.tap();
			await restorePanelType(page);
			await tapNode(page, osc);
			const field = pane(page).getByTestId('param-field-amplitude');
			await field.scrollIntoViewIfNeeded();
			const take = page.getByRole('menuitem', { name: 'Reference selection: variables.desk.level' });
			await longPress(page, await corner(field), take);
			await take.tap();
			await expect.poll(() => param(osc, 'amplitude', 'expression')).toBe('variables.desk.level');
			await expect(
				page.locator(`.svelte-flow__node[data-id="${osc}"]`).getByTestId('node-control-chips'),
				'the node names the element that drives it'
			).toHaveText('desk.level');
		});
	} finally {
		await restorePanelType(page);
		await page.evaluate(async () => {
			const g = (window as any).goofi;
			for (const v of g.query.variables().filter((v: { name: string }) => v.name.startsWith('desk.')))
				await g.commands.removeVariable(v.name);
		});
		await tearDown(page);
	}
});
