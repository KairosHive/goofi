import { test, expect, type Page } from '@playwright/test';
import { waitForApp, resetPatch } from '../lib/app';
import { addNode, frameSummary, selectNode, updateParam, waitForNode } from '../lib/goofi';
import { rawCall } from '../lib/harness';
import { backendDoc } from '../lib/raw';

type Clip = { x: number; y: number; width: number; height: number };

/** A page region's red-channel contrast, its strongest blue-over-red tint (the series colour is
 * blue, the labels and the chrome are grey) and the share of rows carrying that tint. */
async function inspect(page: Page, clip: Clip): Promise<{ contrast: number; tint: number; litRows: number }> {
	const png = (await page.screenshot({ clip })).toString('base64');
	return page.evaluate(async (encoded) => {
		const bytes = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
		const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
		const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
		const ctx = canvas.getContext('2d')!;
		ctx.drawImage(bitmap, 0, 0);
		const { data, width, height } = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
		bitmap.close();
		let low = 255;
		let high = 0;
		let tint = 0;
		let litRows = 0;
		for (let y = 0; y < height; y++) {
			let lit = false;
			for (let x = 0; x < width; x++) {
				const i = (y * width + x) * 4;
				const r = data[i];
				const blue = data[i + 2] - r;
				low = Math.min(low, r);
				high = Math.max(high, r);
				tint = Math.max(tint, blue);
				if (blue > 30) lit = true;
			}
			if (lit) litRows++;
		}
		return { contrast: high - low, tint, litRows: litRows / height };
	}, png);
}

test('the plot surface draws a viewer inside its card, and only while the card shows it', async ({ page }) => {
	// The editor's viewers draw on ONE canvas under the cards. Only pixels can say that a signal
	// arrives as a picture, a collapsed body leaves none behind, and the picture pans with its card.
	const glErrors: string[] = [];
	page.on('console', (m) => {
		if (m.text().includes('INVALID_OPERATION')) glErrors.push(m.text());
	});
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto('/');
	await waitForApp(page);
	try {
		const osc = await addNode(page, 'LFO', [120, 80]);
		await waitForNode(page, osc);
		await page.evaluate((u) => {
			const g = (window as any).goofi;
			g.commands.updateParam(u, 'output', 'mode', 'block');
			g.commands.updateParam(u, 'output', 'sfreq', 64);
		}, osc);
		const card = page.locator(`.svelte-flow__node[data-id="${osc}"]`);
		const body = card.locator('.slot-viewer .body');
		let flat = '';
		let ramp = '';
		await expect(body).toBeVisible();
		await expect.poll(() => frameSummary(page, osc)).not.toBeNull();

		await test.step('a sine draws a band that is not flat', async () => {
			await expect
				.poll(async () => (await inspect(page, (await body.boundingBox())!)).tint)
				.toBeGreaterThan(40);
			const seen = await inspect(page, (await body.boundingBox())!);
			expect(seen.litRows, 'the line sweeps the body, not one row of it').toBeGreaterThan(0.4);
		});

		await test.step('a collapsed viewer draws nothing in its rect', async () => {
			const was = (await body.boundingBox())!;
			await card.getByLabel('toggle viewer').first().click();
			await expect(body).toHaveCount(0);
			const now = (await card.boundingBox())!;
			const top = Math.max(was.y, now.y + now.height + 2);
			const below = { x: was.x, y: top, width: was.width, height: was.y + was.height - top };
			expect(below.height, 'the card shrank away from where the plot was').toBeGreaterThan(20);
			await expect
				.poll(async () => (await inspect(page, below)).contrast, { message: 'the freed room is flat pane' })
				.toBeLessThan(10);
			await card.getByLabel('toggle viewer').first().click();
			await expect(body).toBeVisible();
		});

		await test.step('a zoom keeps the plot inside its card', async () => {
			const before = (await card.boundingBox())!;
			// Zoom about the card's own corner, so it grows in place and stays on screen.
			await page.mouse.move(before.x + 4, before.y + 4);
			await page.mouse.wheel(0, -400);
			await expect
				.poll(async () => (await card.boundingBox())!.width / before.width)
				.toBeGreaterThan(1.2);
			const box = (await body.boundingBox())!;
			await expect
				.poll(async () => (await inspect(page, box)).tint)
				.toBeGreaterThan(40);
			const cardBox = (await card.boundingBox())!;
			// Past the slot pills, which stand off the card's edge and grow with the zoom.
			const right = { x: cardBox.x + cardBox.width + 48, y: box.y, width: 24, height: box.height };
			await expect
				.poll(async () => (await inspect(page, right)).contrast)
				.toBeLessThan(10);
			await page.mouse.wheel(0, 400);
			await expect
				.poll(async () => Math.abs((await card.boundingBox())!.width - before.width))
				.toBeLessThan(4);
		});

		await test.step('a pan keeps the plot inside its card', async () => {
			await expect
				.poll(async () => (await inspect(page, (await body.boundingBox())!)).tint)
				.toBeGreaterThan(40);
			const pane = (await page.locator('.svelte-flow__pane').boundingBox())!;
			const before = (await card.boundingBox())!;
			const from = { x: pane.x + pane.width - 80, y: pane.y + pane.height - 80 };
			await page.mouse.move(from.x, from.y);
			await page.mouse.down();
			await page.mouse.move(from.x + 60, from.y + 40, { steps: 4 });
			await page.mouse.move(from.x + 120, from.y + 80, { steps: 4 });
			await page.mouse.up();
			await expect.poll(async () => (await card.boundingBox())!.x - before.x, { message: 'the card moved with the pan' })
				.toBeGreaterThan(100);
			const box = (await body.boundingBox())!;
			const cardBox = (await card.boundingBox())!;
			await expect
				.poll(async () => (await inspect(page, box)).tint, { message: 'the plot moved with the card' })
				.toBeGreaterThan(40);
			const above = { x: cardBox.x + 20, y: cardBox.y - 30, width: cardBox.width - 40, height: 20 };
			const right = { x: cardBox.x + cardBox.width + 16, y: box.y, width: 24, height: box.height };
			expect((await inspect(page, above)).contrast, 'nothing painted above the card').toBeLessThan(10);
			expect((await inspect(page, right)).contrast, 'nothing painted beside the card').toBeLessThan(10);
		});

		await test.step('a raised card covers the plot beneath it', async () => {
			// A flat line over the sine: the overlap shows the flat card's plot, not the one under it.
			const box = (await body.boundingBox())!;
			flat = await addNode(page, 'LFO', [180, 140]);
			await waitForNode(page, flat);
			await page.evaluate((u) => {
				const g = (window as any).goofi;
				g.commands.updateParam(u, 'lfo', 'amplitude', 0);
				g.commands.updateParam(u, 'output', 'mode', 'block');
				g.commands.updateParam(u, 'output', 'sfreq', 64);
			}, flat);
			const top = page.locator(`.svelte-flow__node[data-id="${flat}"]`);
			const topBody = top.locator('.slot-viewer .body');
			await expect(topBody).toBeVisible();
			await selectNode(page, flat);
			const t = (await topBody.boundingBox())!;
			const x0 = Math.max(box.x, t.x) + 4;
			const y0 = Math.max(box.y, t.y) + 4;
			const overlap = {
				x: x0,
				y: y0,
				width: Math.min(box.x + box.width, t.x + t.width) - 4 - x0,
				height: Math.min(box.y + box.height, t.y + t.height) - 4 - y0
			};
			expect(overlap.width, 'the bodies overlap').toBeGreaterThan(40);
			expect(overlap.height, 'the bodies overlap').toBeGreaterThan(20);
			await expect
				.poll(async () => (await inspect(page, overlap)).tint)
				.toBeGreaterThan(40);
			await expect
				.poll(async () => (await inspect(page, overlap)).litRows)
				.toBeLessThan(0.15);
		});

		await test.step('an image draws after a line plot has gone, with no GL error', async () => {
			await page.evaluate((u) => (window as any).goofi.commands.removeNode(u), flat);
			ramp = await addNode(page, 'graphics:Ramp', [520, 80]);
			await waitForNode(page, ramp);
			const image = page.locator(`.svelte-flow__node[data-id="${ramp}"] .slot-viewer .body`);
			await expect
				.poll(async () => (await inspect(page, (await image.boundingBox())!)).contrast)
				.toBeGreaterThan(100);
			expect(glErrors).toEqual([]);
		});

		await test.step('zoomed out past legibility, a viewer keeps its picture and lets its stream go', async () => {
			const before = (await card.boundingBox())!.width;
			const summary = () => frameSummary(page, osc);
			const wheelTo = async (ratio: (r: number) => boolean, dy: number) => {
				await expect
					.poll(async () => {
						const r = (await card.boundingBox())!.width / before;
						if (!ratio(r)) await page.mouse.wheel(0, dy);
						return ratio(r);
					})
					.toBe(true);
			};
			const at = (await card.boundingBox())!;
			await page.mouse.move(at.x + 4, at.y + 4);
			await wheelTo((r) => r < 0.28, 100);
			await expect.poll(summary).toBeNull();
			expect((await inspect(page, (await body.boundingBox())!)).tint, 'the last frame stays').toBeGreaterThan(40);
			await wheelTo((r) => r > 0.4, -100);
			await expect.poll(summary).not.toBeNull();
		});

		await test.step('a hover near the line reads the point under it, and nothing away from it', async () => {
			const box = (await body.boundingBox())!;
			// The readout is portalled to <body>, so it is found from the page.
			const readout = page.locator('.viewer-hover-readout');
			const x = box.x + box.width / 2;
			// The sine crosses every row at some column, so a column scan meets it; the readout
			// gives the value over the sample index, and marks the point on the line.
			await expect
				.poll(async () => {
					for (let y = box.y + 6; y < box.y + box.height - 6; y += 4) {
						await page.mouse.move(x, y);
						if ((await readout.count()) > 0) return readout.innerText();
					}
					return '';
				})
				.toMatch(/^-?\d[\d.]*\s+x \d+$/);
			await expect(body.locator('.mark')).toHaveCount(1);
			await page.mouse.move(4, 4);
			await expect(readout).toHaveCount(0);
		});

		await test.step('a hover over an image reads the pixel coordinate and its value', async () => {
			const image = page.locator(`.svelte-flow__node[data-id="${ramp}"] .slot-viewer .body`);
			const box = (await image.boundingBox())!;
			await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
			await expect(page.locator('.viewer-hover-readout')).toHaveText(/^-?[\d.]+x \d+y \d+$/);
			await expect(image.locator('.mark')).toHaveCount(0);
		});

		await test.step('the range labels show on a selected card, without a hover', async () => {
			const tick = body.locator('.tick').first();
			await page.mouse.move(4, 4);
			await expect(tick).toHaveCSS('opacity', '0');
			await selectNode(page, osc);
			await page.mouse.move(4, 4);
			await expect(tick).toHaveCSS('opacity', '1');
		});
	} finally {
		await resetPatch(page);
	}
});

test('streams served at the cap paint together, so the page paints at the cap', async ({ page }) => {
	// Each slot served on a phase of its own made the page paint once per stream, and a paint
	// loop re-armed per tick split one tick's streams in two: the counter read twice
	// `system.viewer_fps`. A cap the display outpaces by far shows the second; an upper bound,
	// since a busy machine can only paint less.
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto('/');
	await waitForApp(page);
	try {
		await rawCall(page, 'variable entry edit', { name: 'system.viewer_fps', value: 15 });
		const lfos: string[] = [];
		for (const x of [40, 340, 640]) {
			const n = await addNode(page, 'LFO', [x, 80]);
			await waitForNode(page, n);
			await page.evaluate((u) => (window as any).goofi.commands.updateParam(u, 'common', 'max_frequency', 200), n);
			lfos.push(n);
		}
		// 400 emits on every stream, two seconds or more at 200 Hz, so the HUD's last window saw all three.
		const emitted = async () => Promise.all(lfos.map(async (n) => (await frameSummary(page, n))?.index ?? -1));
		const from = await emitted();
		await expect
			.poll(async () => (await emitted()).every((i, k) => i > from[k] + 400), { message: 'all three stream' })
			.toBe(true);
		const fps = async () => Number((await page.getByText(/^\d+ fps$/).first().textContent())?.split(' ')[0]);
		expect(await fps(), 'paints a second, with three streams at a 15 fps cap').toBeLessThanOrEqual(18);

		// Producers slower than the cap emit at phases of their own; the reducer still serves each
		// frame on the next tick of the grid, so they arrive together and the page paints no more.
		for (const [k, n] of lfos.entries())
			await page.evaluate(([u, f]) => (window as any).goofi.commands.updateParam(u, 'common', 'max_frequency', f), [n, 7 + 4 * k] as const);
		const again = await emitted();
		await expect
			.poll(async () => (await emitted()).every((i, k) => i > again[k] + 20), { message: 'all three stream below the cap' })
			.toBe(true);
		expect(await fps(), 'paints a second, with three streams below the cap').toBeLessThanOrEqual(18);
	} finally {
		await resetPatch(page);
	}
});

test('the brain draws a scalp on the surface, names its channels in the DOM, and turns under its own drag', async ({ page }) => {
	// A value per named channel is a topomap; a channel-by-channel matrix a ring, whose names are
	// text the surface never draws; in 3-D a press on it must not start a node drag, and the ring
	// has no drag, so the same press moves the node as a press on any viewer does.
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto('/');
	await waitForApp(page);
	try {
		const c = await addNode(page, 'signal:Constant', [160, 120]);
		await waitForNode(page, c);
		await updateParam(page, c, 'constant', 'shape', '4');
		const m = await addNode(page, 'signal:Meta', [420, 120]);
		await waitForNode(page, m);
		await updateParam(page, m, 'meta', 'labels', 'Fz,Cz,Pz,Oz');
		await page.evaluate(
			([a, b]) =>
				(window as any).goofi.commands.addLink({ node_out: a, slot_out: 'out', node_in: b, slot_in: 'input' }),
			[c, m]
		);
		await expect.poll(() => frameSummary(page, m)).not.toBeNull();
		const card = page.locator(`.svelte-flow__node[data-id="${m}"]`);
		const feed = card.locator('.slot-viewer .body .viewer-feed');
		const drag = async (): Promise<{ dx: number; dy: number }> => {
			const before = (await card.boundingBox())!;
			const box = (await feed.boundingBox())!;
			await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
			await page.mouse.down();
			await page.mouse.move(box.x + box.width / 2 + 60, box.y + box.height / 2 + 40, { steps: 8 });
			await page.mouse.up();
			const after = (await card.boundingBox())!;
			return { dx: after.x - before.x, dy: after.y - before.y };
		};

		await test.step('a value per channel draws a scalp map, read by a hover inside the head', async () => {
			await rawCall(page, 'node edit', { node: m, viewer: [{ slot: 'out', kind: 'brain', settings: {} }] });
			await expect
				.poll(async () => (await inspect(page, (await feed.boundingBox())!)).contrast, { message: 'the head disc is painted' })
				.toBeGreaterThan(60);
			const box = (await feed.boundingBox())!;
			await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
			await expect(page.locator('.viewer-hover-readout')).toHaveText(/^-?[\d.]+/);
			await page.mouse.move(4, 4);
		});

		await test.step('a channel-by-channel matrix draws a ring, its channel names as text', async () => {
			await updateParam(page, c, 'constant', 'shape', '4,4');
			await expect(feed.locator('.placed')).toHaveCount(4);
			await expect(feed.locator('.placed').first()).toHaveText('Fz');
			await expect(feed).not.toHaveClass(/nodrag/);
			const moved = await drag();
			expect(moved.dx, 'the card followed the pointer').toBeGreaterThan(40);
			expect(moved.dy).toBeGreaterThan(20);
		});

		await test.step('the 3-D brain takes the drag', async () => {
			await rawCall(page, 'node edit', { node: m, viewer: [{ slot: 'out', kind: 'brain', settings: { mode: '3d' } }] });
			await expect(feed, 'the viewer declares its drag once the frame is drawn').toHaveClass(/nodrag/);
			await expect(feed.locator('.placed')).toHaveCount(0);
			expect(await drag()).toEqual({ dx: 0, dy: 0 });
		});
	} finally {
		await resetPatch(page);
	}
});

test('the cog opens a card\'s settings, a click elsewhere closes them, and the header keeps its slot name', async ({ page }) => {
	// The menu portals to <body> from between the cog and the slot name; a dismiss must remove the
	// menu and nothing else of the header. A setting picked in it lands in the document.
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto('/');
	await waitForApp(page);
	try {
		const osc = await addNode(page, 'LFO', [300, 200]);
		await waitForNode(page, osc);
		const card = page.locator(`.svelte-flow__node[data-id="${osc}"]`);
		const name = card.getByTestId('slot-output').first();
		const cog = card.getByTestId('viewer-settings-cog').first();
		const menu = page.getByTestId('viewer-settings-menu');
		await expect(name).toHaveText('out');

		await cog.click();
		await expect(menu).toBeVisible();
		await menu.getByLabel('Auto range').click();
		await expect
			.poll(async () => (await backendDoc(page)).nodes[osc]?.viewers?.out?.settings?.yAuto)
			.toBe(false);

		await page.mouse.click(900, 600);
		await expect(menu).toHaveCount(0);
		await expect(name, 'the dismiss took only the menu').toHaveText('out');
		await expect(cog).toBeVisible();
		await expect(card.locator('.slot-viewer .body')).toBeVisible();
	} finally {
		await resetPatch(page);
	}
});
