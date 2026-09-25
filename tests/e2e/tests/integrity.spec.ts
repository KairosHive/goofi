// One complex scene, built up in stages, swept for structural violations after each.
//
// This is the whole of the app's visual coverage, and deliberately so. It asserts NO design value:
// every rule in `lib/invariants` compares the app against itself or against its own `--hit` token,
// so a restyle is free and only the app falling apart is red — a page that scrolls, text clipped
// away, a control off screen or too small to hit.
//
// It runs in every viewport project, which is what makes it a responsive test: the same scene at
// 1280, at 412 portrait, at 863 landscape and on a tablet, judged by one rule set.

import { test, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { closeAddedTab, closeSplit, splitRight, waitForApp } from '../lib/app';
import { expectIntact } from '../lib/invariants';
import { addNode, frameSummary, selectNode, waitForNode } from '../lib/goofi';
import { rawCall } from '../lib/raw';

/** Open the panel header menu, reveal its content submenu, and answer every row it shows. */
async function panelTypeRows(page: Page): Promise<string[]> {
	await page.getByTestId('panel-header').first().click({ button: 'right' });
	await page.locator('.context-menu .item', { hasText: 'Change content' }).first().hover();
	await expect(page.locator('.context-menu .item').nth(6)).toBeVisible();
	const rows = (await page.locator('.context-menu .item').allTextContents()).map((t) => t.trim());
	await page.keyboard.press('Escape');
	return rows;
}

/** Switch the first panel to the type named in its own switcher. */
async function choosePanelType(page: Page, name: string): Promise<void> {
	await page.getByTestId('panel-header').first().click({ button: 'right' });
	await page.locator('.context-menu .item', { hasText: 'Change content' }).first().hover();
	await page.locator('.context-menu .item', { hasText: new RegExp(`^\\s*${name}\\s*$`) })
		.first()
		.click();
	await expect(page.locator('.context-menu')).toHaveCount(0);
}

/** Everything this scene put on the backend, so the next spec meets a pristine workspace. */
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

test('a patch under construction holds together at every stage', async ({ page }) => {
	const thrown: string[] = [];
	page.on('pageerror', (e) => thrown.push(String(e)));
	await page.goto('/');
	await waitForApp(page);
	const wide = (page.viewportSize()?.width ?? 0) >= 900;
	try {
		await test.step('an empty workspace', async () => {
			await expectIntact(page, 'the empty app');
		});

		await test.step('the node menu keeps search focused across categories', async () => {
			await page.locator('.svelte-flow__pane').first().dblclick();
			const menu = page.getByRole('dialog', { name: 'Add node', exact: true });
			const search = page.getByTestId('add-menu-search');
			await expect(search).toBeFocused();
			const tabs = page.getByTestId('add-menu-tabs').getByRole('tab');
			for (const tab of await tabs.all()) {
				await tab.click();
				await expect(tab).toHaveAttribute('aria-selected', 'true');
				await expect(search).toBeFocused();
			}
			await page.keyboard.type('LFO');
			await expect(search).toHaveValue('LFO');
			await search.press('Tab');
			await expect(search).toBeFocused();
			await search.press('Shift+Tab');
			await expect(search).toBeFocused();
			await expectIntact(page, 'the node menu');
			await search.press('Escape');
			await expect(menu).toHaveCount(0);
		});

		let osc = '';
		let buf = '';
		await test.step('two nodes on the canvas, wired', async () => {
			osc = await addNode(page, 'LFO', [40, 40]);
			await waitForNode(page, osc);
			buf = await addNode(page, 'Buffer', [320, 40]);
			await waitForNode(page, buf);
			await page.evaluate(
				([a, b]) =>
					(window as any).goofi.commands.addLink({
						node_out: a,
						slot_out: 'out',
						node_in: b,
						slot_in: 'input'
					}),
				[osc, buf]
			);
			await expectIntact(page, 'a wired pair');
		});

		await test.step('a viewer streaming real frames', async () => {
			// A node that is RUNNING, not merely present: a viewer sizes itself around live data, and
			// an empty one cannot overflow the way a full one can.
			await expect.poll(() => frameSummary(page, buf), { message: 'frames reached the tab' }).not.toBeNull();
			await expectIntact(page, 'a streaming viewer');
			// A card under a live runtime never blinks out: every delta re-derives the flow, and a
			// node SvelteFlow has not measured is hidden until it is — a press then lands on the pane.
			await page.evaluate((u) => {
				const card = document.querySelector(`.svelte-flow__node[data-id="${u}"]`) as HTMLElement;
				const w = window as any;
				w.blinks = 0;
				w.blinkWatch = new MutationObserver(() => {
					if (getComputedStyle(card).visibility === 'hidden') w.blinks++;
				});
				w.blinkWatch.observe(card, { attributes: true, attributeFilter: ['style'] });
			}, buf);
			// Watched across 60 of the buffer's emits, two seconds at the default rate.
			const from = (await frameSummary(page, buf)).index;
			await expect.poll(async () => (await frameSummary(page, buf))?.index).toBeGreaterThan(from + 60);
			const blinks = await page.evaluate(() => {
				const w = window as any;
				w.blinkWatch.disconnect();
				return w.blinks;
			});
			expect(blinks, 'the streaming card never went hidden').toBe(0);
		});

		await test.step('a spectrum on a log axis, streaming', async () => {
			// A PSD floor sits far below 1e-22 — a range a naive log tick walk never terminates on.
			// A spectrum needs a run of samples that knows its own rate, so the source emits blocks
			// and the buffer already on the canvas is what gathers them.
			await rawCall(page, 'node param edit', { node: osc, param: 'output/mode', value: 'block' });
			const psd = await addNode(page, 'Psd', [320, 240]);
			await waitForNode(page, psd);
			await page.evaluate(
				([a, b]) =>
					(window as any).goofi.commands.addLink({
						node_out: a,
						slot_out: 'out',
						node_in: b,
						slot_in: 'input'
					}),
				[buf, psd]
			);
			await rawCall(page, 'node edit', {
				node: psd,
				viewer: [{ slot: 'out', kind: 'line', settings: { logY: true } }]
			});
			await expect.poll(() => frameSummary(page, psd), { message: 'spectra reached the tab' }).not.toBeNull();
			await expect(
				page.locator(`.svelte-flow__node[data-id="${psd}"] .tick`).first(),
				'the spectrum painted its log axis'
			).toBeVisible();
			await expectIntact(page, 'a streaming spectrum');
		});

		await test.step('the inspector open over it, with every param group rendered', async () => {
			await selectNode(page, osc);
			await expect(page.getByTestId('auto-side-panel')).toHaveClass(/open/);
			await expectIntact(page, 'the inspector open');
			const tabs = page.getByTestId('param-tabs');
			for (const name of await tabs.getByRole('tab').allTextContents()) {
				await tabs.getByRole('tab', { name }).click();
				await expectIntact(page, `the inspector on its ${name} group`);
			}
		});

		await test.step('a long name does not burst the boxes that carry it', async () => {
			// The scene's one adversarial input. Everything else here is well-behaved content, and a
			// layout only falls apart when something in it is bigger than the design imagined. Renamed
			// through the inspector's own header, because there is no rename on the command façade.
			const panel = page.getByTestId('auto-side-panel');
			await panel.getByTestId('node-name').click();
			const input = panel.getByTestId('node-name-input');
			await input.fill('anodenamefarlongerthananyheaderwasdrawntohold');
			await input.press('Enter');
			await expect
				.poll(() => page.evaluate((u) => (window as any).goofi.query.node(u)?.name, osc))
				.toContain('farlonger');
			await expectIntact(page, 'a very long node name');
		});

		if (wide) {
			await test.step('a second panel beside the first', async () => {
				// The inspector is parked first: it is an overlay ON the editor, and a split under an
				// open one puts the new panel's header beneath it.
				await page.getByTestId('auto-side-panel').getByTestId('inspector-close').click();
				await expect(page.getByTestId('auto-side-panel')).not.toHaveClass(/open/);
				await splitRight(page);
				await expectIntact(page, 'a split workspace');
				await closeSplit(page);
			});
		}

		await test.step('two slots armed for recording', async () => {
			// The recorder panel is swept with CONTENT: a row per armed stream, which is the part of
			// it a restyle can crush. The rows ride the document, so the walk below meets them.
			await page.evaluate(
				([a, b]) => {
					const g = (window as any).goofi;
					return Promise.all([g.commands.armSlot(a, 'out'), g.commands.armSlot(b, 'out')]);
				},
				[osc, buf]
			);
			await expect
				.poll(() => page.evaluate(() => (window as any).goofi.query.armed().length))
				.toBe(2);

			// An arm that changes nothing must leave the undo stack alone: a phantom step there pops
			// against a manager command that is not here, and takes the user's last real edit with it.
			await page.evaluate((a) => (window as any).goofi.commands.setNodePos(a, [60, 60]), osc);
			await expect(page.getByTestId('topbar-undo')).toHaveAttribute('title', /Move/);
			await page.evaluate((a) => (window as any).goofi.commands.armSlot(a, 'out'), osc);
			await expect(page.getByTestId('topbar-undo')).toHaveAttribute('title', /Move/);
			await page.getByTestId('topbar-undo').click();
			await expect
				.poll(() => page.evaluate(() => (window as any).goofi.query.armed().length))
				.toBe(2);
		});

		await test.step('a second workspace tab', async () => {
			await page.evaluate(() => (window as any).goofi.commands.addTab());
			await expect(page.getByTestId('workspace-tabs').locator('.ui-tab')).toHaveCount(2);
			await expectIntact(page, 'a second tab');
			await closeAddedTab(page);
		});

		if (wide) {
			await test.step('every panel type the build offers, one after another', async () => {
				// The sweep names no element, so a panel type added later is covered the day it renders —
				// but only if the scene MOUNTS it. The list comes from the panel's own switcher rather
				// than from a literal here, so a new type joins this walk by existing.
				const CHROME = ['split', 'maximize', 'change content', 'close'];
				const names = (await panelTypeRows(page)).filter(
					(n) => !CHROME.some((c) => n.toLowerCase().includes(c))
				);
				expect(names.length, 'the switcher offers types to walk').toBeGreaterThan(3);
				expect(names).toContain('Inspector');
				expect(names).not.toContain('Parameters');
				expect(names).not.toContain('Metadata');
				for (const name of names) {
					await choosePanelType(page, name);
					if (name === 'Inspector') {
						await page.getByTestId('panel-node').locator('select').selectOption(osc);
						await expect(page.locator('.param-form')).toBeVisible();
						await expect(page.getByText('Metadata', { exact: true })).toBeVisible();
						await expect(page.locator('.meta-field').first()).toBeVisible();
					}
					if (name === 'Recorder')
						await expect(page.getByTestId('recorder-stream').first()).toBeVisible();
					await expectIntact(page, `the ${name} panel`);
				}
				await choosePanelType(page, 'Node Editor');
			});
		}

		await test.step('and the renderer threw nothing along the way', async () => {
			expect(thrown).toEqual([]);
		});
	} finally {
		await tearDown(page);
	}
});

test('the primitive gallery holds together', async ({ page }) => {
	// `/dev/ui` renders one sample of every primitive the `$lib/ui` barrel exports, so sweeping it
	// asks the integrity question of the whole component library at once — including the primitives
	// no screen in the app happens to be showing right now. It is served only under `--debug`, which
	// is how the fleet spawns.
	await page.goto('/dev/ui');
	await expect(page.getByTestId('ui-button-default-md')).toBeVisible();
	await expectIntact(page, 'the primitive gallery');
});


test('parameter groups keep readable widths and scroll to the last group', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		// Nine groups of its own, wider together than the inspector, beside the one every node has.
		const groups = ['texture', 'warp', 'octaves', 'palette', 'lighting', 'feedback', 'camera', 'timing', 'output'];
		const status = (await rawCall(page, 'session status')).result;
		const source = path.join(status.workspace, 'nodes_signal', 'many_groups.py');
		fs.mkdirSync(path.dirname(source), { recursive: true });
		const params = groups.map((g) => `"${g}": {"amount": goofi.FloatParam(0.5, 0.0, 1.0)}`).join(', ');
		fs.writeFileSync(source, `import goofi\nclass ManyGroups(goofi.Node):\n    OUTPUTS = {"out": goofi.DataType.ARRAY}\n    PARAMS = {${params}}\n`);
		expect((await rawCall(page, 'library refresh')).error).toBeUndefined();
		const uid = await addNode(page, 'ManyGroups');
		await waitForNode(page, uid);
		await selectNode(page, uid);
		const strip = page.getByTestId('param-tabs');
		const tabs = strip.getByRole('tab');
		await expect(tabs).toHaveCount(groups.length + 1);
		const sizes = await strip.evaluate((el) => ({
			width: el.clientWidth,
			content: el.scrollWidth,
			minimum: 3 * parseFloat(getComputedStyle(document.documentElement).fontSize),
			tabs: Array.from(el.querySelectorAll('[role="tab"]'), (tab) => {
				const label = tab.querySelector('.ui-tab-label')!;
				const range = document.createRange();
				range.selectNodeContents(label);
				const style = getComputedStyle(tab);
				return {
					width: tab.getBoundingClientRect().width,
					natural: range.getBoundingClientRect().width + parseFloat(style.paddingLeft) + parseFloat(style.paddingRight)
				};
			})
		}));
		if (sizes.minimum * sizes.tabs.length > sizes.width) {
			expect(sizes.content).toBeGreaterThan(sizes.width);
		}
		for (const tab of sizes.tabs) {
			expect(tab.width).toBeGreaterThanOrEqual(sizes.minimum - 1);
			expect(tab.width).toBeLessThanOrEqual(Math.max(sizes.minimum, tab.natural) + 1);
		}
		await tabs.first().focus();
		await page.keyboard.press('End');
		await expect(tabs.last()).toHaveAttribute('aria-selected', 'true');
		await expect(tabs.last()).toBeInViewport();
		if (sizes.content > sizes.width) {
			await strip.evaluate((el) => el.scrollTo({ left: el.scrollWidth }));
			await expect.poll(() => strip.evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
		}
		await expectIntact(page, 'the last parameter group after scrolling');
	} finally {
		await tearDown(page);
	}
});

test('a dropdown shows and hides the params, sections and group that depend on it', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		// Three sections of `filter`: the second shows only for `iir`, and `ripple` only while the
		// shown `order` is 4 or 8. The `iir` group shows only for `iir` too.
		const status = (await rawCall(page, 'session status')).result;
		const source = path.join(status.workspace, 'nodes_signal', 'shown_by_mode.py');
		fs.mkdirSync(path.dirname(source), { recursive: true });
		fs.writeFileSync(source, [
			'import goofi',
			'class ShownByMode(goofi.Node):',
			'    OUTPUTS = {"out": goofi.DataType.ARRAY}',
			'    PARAMS = {',
			'        "filter": [',
			'            {"mode": goofi.StringParam("fir", options=["fir", "iir"]), "taps": goofi.IntParam(64, 1, 512, show=("mode", ["fir"]))},',
			'            {"order": goofi.IntParam(4, 2, 8, options=[2, 4, 8], show=("mode", ["iir"])), "ripple": goofi.FloatParam(0.5, 0.0, 1.0, show=("order", [4, 8]))},',
			'            {"gain": goofi.FloatParam(1.0, 0.0, 2.0)},',
			'        ],',
			'        "iir": {"q": goofi.FloatParam(0.7, 0.1, 10.0, show=("filter.mode", ["iir"]))},',
			'    }',
			''
		].join('\n'));
		expect((await rawCall(page, 'library refresh')).error).toBeUndefined();
		const uid = await addNode(page, 'ShownByMode');
		await waitForNode(page, uid);
		await selectNode(page, uid);
		const tabs = page.getByTestId('param-tabs').getByRole('tab');
		const row = (key: string) => page.locator(`[data-param-key="${key}"]`);
		const lines = page.getByTestId('param-rows').getByTestId('param-section-break');

		// A section with no shown param draws nothing, so one line parts the two shown ones.
		await expect(tabs).toHaveText(['filter', 'common']);
		await expect(row('filter/taps')).toBeVisible();
		await expect(row('filter/order')).toHaveCount(0);
		await expect(row('filter/ripple'), 'a param shown by a hidden one hides with it').toHaveCount(0);
		await expect(lines).toHaveCount(1);

		await page.getByTestId('param-field-mode').locator('select').selectOption('iir');
		await expect(row('filter/taps')).toHaveCount(0);
		await expect(row('filter/order')).toBeVisible();
		await expect(row('filter/ripple')).toBeVisible();
		await expect(lines).toHaveCount(2);
		await expect(tabs).toHaveText(['filter', 'iir', 'common']);
		await expectIntact(page, 'a group parted into sections');

		// A search does not reach a hidden param.
		const search = page.getByTestId('param-search');
		await search.fill('taps');
		await expect(page.getByTestId('param-no-matches')).toBeVisible();
		await search.fill('');

		// The fronted group loses every param to the dropdown, so the front falls back to the first.
		await tabs.getByText('iir', { exact: true }).click();
		await expect(row('iir/q')).toBeVisible();
		await rawCall(page, 'node param edit', { node: uid, param: 'filter/mode', value: 'fir' });
		await expect(tabs).toHaveText(['filter', 'common']);
		await expect(row('filter/taps')).toBeVisible();
		await expect(lines).toHaveCount(1);
	} finally {
		await tearDown(page);
	}
});
