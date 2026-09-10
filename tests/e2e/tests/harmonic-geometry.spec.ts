import { test, expect, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { waitForApp, resetPatch } from '../lib/app';
import { rawCall } from '../lib/raw';
import { REPO_ROOT } from '../playwright.config';

const folder = path.join(REPO_ROOT, 'node-bundles', 'harmonic-geometry', 'examples');
const textures = ['jade', 'brushed metal', 'woven silk', 'porous stone', 'sand', 'dunes', 'lichen', 'coral', 'cells', 'spores', 'pollen', 'plankton'];
const recipes = JSON.parse(fs.readFileSync(path.join(folder, 'recipes.json'), 'utf8')) as Array<{
	file: string; title: string; nodes: number; views: [string, string, string][];
}>;

async function imageContrast(page: Page): Promise<number> {
	const png = await page.locator('.vp-body canvas:visible').first().screenshot();
	return page.evaluate(async (encoded) => {
		const bytes = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
		const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
		const canvas = new OffscreenCanvas(32, 32);
		const ctx = canvas.getContext('2d')!;
		// The center avoids letterboxing: a flat startup frame must not pass.
		ctx.drawImage(bitmap, bitmap.width/4, bitmap.height/4, bitmap.width/2, bitmap.height/2, 0, 0, 32, 32);
		bitmap.close();
		const pixels = ctx.getImageData(0, 0, 32, 32).data;
		let low = 255, high = 0;
		for (let i = 0; i < pixels.length; i += 4) { low = Math.min(low, pixels[i]); high = Math.max(high, pixels[i]); }
		return high-low;
	}, png.toString('base64'));
}

async function imageMovement(page: Page, previous: string): Promise<number> {
	const current = (await page.locator('.vp-body canvas:visible').first().screenshot()).toString('base64');
	return page.evaluate(async ([previous, current]) => {
		const read = async (encoded: string) => {
			const bytes = Uint8Array.from(atob(encoded), c => c.charCodeAt(0));
			const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
			const canvas = new OffscreenCanvas(64, 64);
			const ctx = canvas.getContext('2d')!;
			ctx.drawImage(bitmap, 0, 0, 64, 64);
			bitmap.close();
			return ctx.getImageData(0, 0, 64, 64).data;
		};
		const [a, b] = await Promise.all([read(previous), read(current)]);
		let difference = 0;
		for (let i = 0; i < a.length; i += 4) difference += Math.abs(a[i]-b[i]);
		return difference/(64*64);
	}, [previous, current]);
}

test('the harmonic geometry cookbook opens as live dashboards with usable controls', async ({ page }) => {
	test.setTimeout(360_000);
	page.setDefaultTimeout(15_000);
	const thrown: string[] = [];
	page.on('pageerror', (e) => thrown.push(String(e)));
	await page.setViewportSize({ width: 1440, height: 1000 });
	await page.goto('/');
	await waitForApp(page);
	try {
		for (const recipe of recipes) {
			await test.step(recipe.title, async () => {
				const loaded = await rawCall(page, 'session load', { path: path.join(folder, recipe.file) });
				expect(loaded.error, JSON.stringify(loaded)).toBeUndefined();
				await expect.poll(() => page.evaluate(() => (window as any).goofi.query.graph().nodes.length)).toBe(recipe.nodes);
				await page.getByRole('tab', { name: 'Play Close tab', exact: true }).click();
				await expect(page.getByTestId('control-panel')).toBeVisible();
				for (const [index, [name, slot, kind]] of recipe.views.entries()) {
					if (index) await page.getByTestId('workspace-tabs').locator('.ui-tab').filter({ hasText: name }).click();
					await expect.poll(() => page.evaluate(({ name, slot }) => {
						const g = (window as any).goofi;
						const node = g.query.graph().nodes.find((n: any) => n.name === name);
						return node && g.query.frameSummary(node.uid, slot) !== null;
					}, { name, slot }), { timeout: 45_000, message: `${recipe.file}: ${name}.${slot} reaches the browser` }).toBe(true);
					if (kind !== 'string') await expect(page.locator('.vp-body canvas').first()).toBeVisible();
					if (!index) {
						if (slot === 'out') await expect.poll(() => imageContrast(page),
							{ timeout: 45_000, message: 'The plate viewer shows a pattern after its shader inputs arrive' }).toBeGreaterThan(8);
						if (slot === 'dashboard') await expect.poll(() => page.evaluate(({ name, slot }) => {
							const g = (window as any).goofi;
							const node = g.query.graph().nodes.find((n: any) => n.name === name);
							const frame = g.query.frameSummary(node.uid, slot);
							return frame && (frame.reducedLength === undefined || frame.reducedLength >= 300_000);
						}, { name, slot })).toBe(true);
						await page.screenshot({ path: path.join(folder, 'assets', recipe.file.replace('.gfi', '-browser.png')) });
					}
				}
				await page.getByRole('tab', { name: 'Play Close tab', exact: true }).click();
				const auto = page.getByTestId('control-geometry-auto');
				await auto.getByRole('checkbox').uncheck();
				await expect.poll(async () => {
					const state = await rawCall(page, 'global list');
					return state.result?.globals?.find((g: any) => g.name === 'geometry.auto')?.value;
				}).toBe(false);
				const slider = page.getByTestId('control-geometry-mix').getByRole('slider');
				await slider.focus();
				await page.keyboard.press('End');
				await expect.poll(async () => {
					const state = await rawCall(page, 'global list');
					return state.result?.globals?.find((g: any) => g.name === 'geometry.mix')?.value;
				}).toBe(1);
				await expect.poll(async () => (await rawCall(page, 'session status')).result.errors,
					{ message: `${recipe.file}: control expressions settle without node errors` }).toEqual([]);
				if (recipe.file.startsWith('09-')) {
					for (const [name, end] of [['relief', 0.45], ['light', 3.14]] as const) {
						const control = page.getByTestId(`control-geometry-${name}`).getByRole('slider');
						await control.scrollIntoViewIfNeeded();
						await control.focus();
						await page.keyboard.press('End');
						await expect.poll(async () => {
							const state = await rawCall(page, 'global list');
							return state.result?.globals?.find((g: any) => g.name === `geometry.${name}`)?.value;
						}).toBe(end);
					}
					await expect.poll(async () => (await rawCall(page, 'session status')).result.errors).toEqual([]);
					const performance = await page.evaluate(() => {
						const g = (window as any).goofi;
						const node = g.query.graph().nodes.find((n: any) => n.name === 'jade');
						return { shape: g.query.frameSummary(node.uid, 'out')?.shape, wireFps: g.query.arrivalRate(node.uid, 'out') };
					});
					await test.info().attach('jade-preview', { body: JSON.stringify(performance), contentType: 'application/json' });
				}
			});
		}
		// One representative patch in both tablet orientations. Controls scroll inside their panel.
		await rawCall(page, 'session load', { path: path.join(folder, recipes[0].file) });
		for (const size of [{ width: 820, height: 1180 }, { width: 1180, height: 820 }]) {
			await page.setViewportSize(size);
			await expect(page.getByTestId('control-geometry-mix')).toBeVisible();
			await expect(page.locator('.vp-body canvas').first()).toBeVisible();
			const bounds = await page.evaluate(() => ({ width: document.documentElement.scrollWidth, height: document.documentElement.scrollHeight }));
			expect(bounds.width).toBeLessThanOrEqual(size.width);
			expect(bounds.height).toBeLessThanOrEqual(size.height);
		}
		expect(thrown).toEqual([]);
	} finally {
		await resetPatch(page);
	}
});

test('jade fills the window and its texture controls morph independently', async ({ page }) => {
	test.setTimeout(120_000);
	const errors: string[] = [];
	page.on('pageerror', (e) => errors.push(String(e)));
	await page.goto('/');
	await waitForApp(page);
	try {
		const loaded = await rawCall(page, 'session load', { path: path.join(folder, '09-jade-resonance.gfi') });
		expect(loaded.error, JSON.stringify(loaded)).toBeUndefined();
		const canvasTab = page.getByRole('tab', { name: 'Canvas Close tab', exact: true });
		await expect(page.getByRole('tab', { name: 'Play Close tab', exact: true })).toHaveAttribute('aria-selected', 'true');
		await expect(page.getByTestId('control-geometry-textureA').getByRole('combobox')).toHaveValue('sand');
		await canvasTab.click();
		const canvas = page.locator('.vp-body canvas:visible').first();
		await expect.poll(() => page.evaluate(() => {
			const g = (window as any).goofi;
			const node = g.query.graph().nodes.find((n: any) => n.name === 'jade');
			return node && g.query.frameSummary(node.uid, 'out')?.shape;
		}), { timeout: 45_000 }).toEqual([512, 512, 4]);
		for (const size of [{ width: 1440, height: 1000 }, { width: 820, height: 1180 }]) {
			await page.setViewportSize(size);
			await expect(canvas).toBeVisible();
			await expect(canvas).toHaveCSS('object-fit', 'fill');
			const box = (await canvas.boundingBox())!;
			expect(box.width).toBeGreaterThan(size.width*0.9);
			expect(box.height).toBeGreaterThan(size.height*0.75);
			const bounds = await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.scrollHeight]);
			expect(bounds[0]).toBeLessThanOrEqual(size.width);
			expect(bounds[1]).toBeLessThanOrEqual(size.height);
		}
		await page.setViewportSize({ width: 1440, height: 1000 });
		await page.getByRole('tab', { name: 'Play Close tab', exact: true }).click();
		for (const name of ['auto', 'textureAuto']) await page.getByTestId(`control-geometry-${name}`).getByRole('checkbox').uncheck();
		for (const [name, value] of [['textureA', 'cells'], ['textureB', 'plankton']]) {
			const select = page.getByTestId(`control-geometry-${name}`).getByRole('combobox');
			await expect(select.locator('option')).toHaveText(textures);
			await select.selectOption(value);
		}
		const slider = page.getByTestId('control-geometry-textureMix').getByRole('slider');
		await slider.focus();
		await page.keyboard.press('End');
		await expect.poll(async () => {
			const state = await rawCall(page, 'global list');
			return state.result.globals.filter((g: any) => ['geometry.textureMix', 'geometry.mix'].includes(g.name)).map((g: any) => [g.name, g.value]);
		}).toEqual([['geometry.mix', 0.3], ['geometry.textureMix', 1]]);
		await expect.poll(async () => (await rawCall(page, 'session status')).result.errors).toEqual([]);
		// Return to the saved finish for the cookbook picture, with manual controls held.
		await page.getByTestId('control-geometry-textureA').getByRole('combobox').selectOption('sand');
		await page.getByTestId('control-geometry-textureB').getByRole('combobox').selectOption('lichen');
		await slider.focus();
		await page.keyboard.press('Home');
		await canvasTab.click();
		await expect.poll(() => imageContrast(page), { timeout: 30_000 }).toBeGreaterThan(15);
		await page.screenshot({ path: path.join(folder, 'assets', '09-jade-resonance-browser.png') });
		expect(errors).toEqual([]);
	} finally {
		await resetPatch(page);
	}
});

test('living ratios modulate the organic field with a visible trace and pause control', async ({ page }) => {
	test.setTimeout(120_000);
	await page.setViewportSize({ width: 1440, height: 1000 });
	await page.goto('/');
	await waitForApp(page);
	try {
		const loaded = await rawCall(page, 'session load', { path: path.join(folder, '10-living-ratios.gfi') });
		expect(loaded.error, JSON.stringify(loaded)).toBeUndefined();
		await expect(page.getByTestId('control-geometry-running').getByRole('checkbox')).toBeChecked();
		await expect(page.getByTestId('control-geometry-auto').getByRole('checkbox')).not.toBeChecked();
		await expect(page.getByTestId('control-geometry-approach').getByRole('slider')).toHaveValue('0');
		await expect(page.getByTestId('control-geometry-glide').getByRole('slider')).toHaveValue('1');
		await expect(page.getByTestId('control-geometry-mapping').getByRole('combobox')).toHaveValue('chord pairs');
		await expect(page.getByTestId('control-geometry-motion').getByRole('combobox')).toHaveValue('fields');
		await expect(page.getByTestId('control-geometry-squareSymmetry').getByRole('combobox')).toHaveValue('d4_max');
		await expect.poll(() => page.locator('.vp-body canvas:visible').count()).toBeGreaterThanOrEqual(2);
		await expect.poll(() => imageContrast(page), { timeout: 45_000 }).toBeGreaterThan(15);
		const still = (await page.locator('.vp-body canvas:visible').first().screenshot()).toString('base64');
		await expect.poll(() => imageMovement(page, still), { timeout: 8_000,
			message: 'The Chladni picture visibly changes while the texture stays fixed' }).toBeGreaterThan(4);
		const sample = async () => (await rawCall(page, 'node snapshot', { output: 'chordTuning/out' })).result?.range?.mean;
		await expect.poll(sample).toBeGreaterThan(1);
		const first = await sample();
		await expect.poll(sample, { timeout: 12_000 }).not.toBe(first);
		await page.getByTestId('control-geometry-running').getByRole('checkbox').uncheck();
		await expect.poll(async () => (await rawCall(page, 'global list')).result.globals.find((g: any) => g.name === 'geometry.running').value).toBe(false);
		await expect.poll(async () => (await rawCall(page, 'session status')).result.errors).toEqual([]);
		await page.screenshot({ path: path.join(folder, 'assets', '10-living-ratios-browser.png') });
		await page.getByRole('tab', { name: 'nodalLines Close tab', exact: true }).click();
		await expect.poll(() => imageContrast(page)).toBeGreaterThan(15);
		await page.getByRole('tab', { name: 'modes Close tab', exact: true }).click();
		await expect(page.locator('.vp-body')).toContainText('integer chord');
		await expect(page.locator('.vp-body')).toContainText('pairs');
	} finally {
		await resetPatch(page);
	}
});

test('the illustrated cookbook reads on a phone and filters its geometry atlas', async ({ page }, info) => {
	const errors: string[] = [];
	page.on('pageerror', (e) => errors.push(String(e)));
	await page.setViewportSize({ width: 1440, height: 1050 });
	await page.goto(pathToFileURL(path.join(folder, 'Cookbook.html')).href);
	await expect(page.locator('.recipe')).toHaveCount(recipes.length);
	await expect(page.locator('.specimen')).toHaveCount(46);
	await page.screenshot({ path: info.outputPath('cookbook-desktop.png') });
	await page.getByRole('searchbox', { name: 'Filter geometry methods' }).fill('knot');
	await expect(page.locator('.specimen:visible')).toHaveCount(1);
	await page.setViewportSize({ width: 390, height: 844 });
	await page.evaluate(() => scrollTo(0, 0));
	await expect(page.locator('h1')).toBeVisible();
	const width = await page.evaluate(() => document.documentElement.scrollWidth);
	expect(width).toBeLessThanOrEqual(390);
	await page.screenshot({ path: info.outputPath('cookbook-phone.png') });
	expect(errors).toEqual([]);
});
