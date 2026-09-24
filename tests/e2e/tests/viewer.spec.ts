import { test, expect, type Page } from '@playwright/test';
import { waitForApp, resetPatch } from '../lib/app';
import { addNode, selectNode, waitForNode } from '../lib/goofi';

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
	test.setTimeout(90_000);
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
		await expect(body).toBeVisible();
		await expect
			.poll(() => page.evaluate((u) => (window as any).goofi.query.frameSummary(u, 'out') !== null, osc), {
				timeout: 30_000
			})
			.toBe(true);

		await test.step('a sine draws a band that is not flat', async () => {
			await expect
				.poll(async () => (await inspect(page, (await body.boundingBox())!)).tint, { timeout: 20_000 })
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
				.poll(async () => (await card.boundingBox())!.width / before.width, { timeout: 10_000 })
				.toBeGreaterThan(1.2);
			const box = (await body.boundingBox())!;
			await expect
				.poll(async () => (await inspect(page, box)).tint, { timeout: 20_000 })
				.toBeGreaterThan(40);
			const cardBox = (await card.boundingBox())!;
			// Past the slot pills, which stand off the card's edge and grow with the zoom.
			const right = { x: cardBox.x + cardBox.width + 48, y: box.y, width: 24, height: box.height };
			await expect
				.poll(async () => (await inspect(page, right)).contrast, { timeout: 10_000 })
				.toBeLessThan(10);
			await page.mouse.wheel(0, 400);
			await expect
				.poll(async () => Math.abs((await card.boundingBox())!.width - before.width), { timeout: 10_000 })
				.toBeLessThan(4);
		});

		await test.step('a pan keeps the plot inside its card', async () => {
			await expect
				.poll(async () => (await inspect(page, (await body.boundingBox())!)).tint, { timeout: 20_000 })
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
			const flat = await addNode(page, 'LFO', [180, 140]);
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
				.poll(async () => (await inspect(page, overlap)).tint, { timeout: 20_000 })
				.toBeGreaterThan(40);
			await expect
				.poll(async () => (await inspect(page, overlap)).litRows, { timeout: 10_000 })
				.toBeLessThan(0.15);
		});

		await test.step('zoomed out past legibility, a viewer keeps its picture and lets its stream go', async () => {
			const before = (await card.boundingBox())!.width;
			const summary = () => page.evaluate((u) => (window as any).goofi.query.frameSummary(u, 'out'), osc);
			const wheelTo = async (ratio: (r: number) => boolean, dy: number) => {
				await expect
					.poll(async () => {
						const r = (await card.boundingBox())!.width / before;
						if (!ratio(r)) await page.mouse.wheel(0, dy);
						return ratio(r);
					}, { timeout: 10_000 })
					.toBe(true);
			};
			const at = (await card.boundingBox())!;
			await page.mouse.move(at.x + 4, at.y + 4);
			await wheelTo((r) => r < 0.28, 100);
			await expect.poll(summary, { timeout: 10_000 }).toBeNull();
			expect((await inspect(page, (await body.boundingBox())!)).tint, 'the last frame stays').toBeGreaterThan(40);
			await wheelTo((r) => r > 0.4, -100);
			await expect.poll(summary, { timeout: 10_000 }).not.toBeNull();
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
