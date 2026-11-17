// The timeline panel: tracks stacked on one time axis, hidden per panel and shown again from the
// bar's list, a track's menu with its five entries, a delete that asks first, a note and a keyframe
// made by a double click and moved by a drag, and the clock steered from the bar. On a phone, a
// held finger is the door onto the menu and a double tap makes the note.

import { test, expect, type Locator, type Page } from '@playwright/test';
import { closeSplit, restorePanelType, splitRight, waitForApp } from '../lib/app';
import { expectIntact } from '../lib/invariants';
import { backendDoc, rawCall } from '../lib/raw';
import { touchSession } from '../lib/touch';

/** The manager's track named `name`, with its id. */
async function trackNamed(page: Page, name: string): Promise<any> {
	const found = Object.entries<any>((await backendDoc(page)).timeline ?? {}).find(([, t]) => t.name === name);
	return found && { id: found[0], ...found[1] };
}
const clock = async (page: Page, name: string): Promise<number> =>
	(await rawCall(page, 'variable list')).result.variables.find((v: any) => v.name === `clock.${name}`)?.value;

/** A drag of `what` by `dx`, `dy` pixels, from its centre. */
async function dragBy(page: Page, what: Locator, dx: number, dy: number): Promise<void> {
	const box = (await what.boundingBox())!;
	const at = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
	await page.mouse.move(at.x, at.y);
	await page.mouse.down();
	await page.mouse.move(at.x + dx, at.y + dy, { steps: 6 });
	await page.mouse.up();
}

test('tracks are stacked, hidden per panel, edited by gesture, and play on the clock the bar steers', async ({ page, hasTouch }) => {
	test.skip(hasTouch, 'the mouse walk; the finger has the test below');
	await page.goto('/');
	await waitForApp(page);
	const menuItem = (label: string): Locator => page.locator('.context-menu .item', { hasText: label }).first();
	try {
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'timeline');
		});
		const panel = page.getByTestId('timeline-panel').first();
		await expect(panel).toContainText('No tracks');

		// The bar's + adds a track of either kind; the tracks stack in the order they were made.
		for (const kind of ['Note Track', 'Curve Track', 'Note Track']) {
			await panel.getByTestId('timeline-add').click();
			await menuItem(kind).click();
		}
		const [keys, fade, bass] = ['track0', 'track1', 'track2'].map((n) => panel.getByTestId(`track-${n}`));
		await expect(bass).toBeVisible();
		const tops = await Promise.all([keys, fade, bass].map(async (t) => (await t.boundingBox())!.y));
		expect(tops[0] < tops[1] && tops[1] < tops[2], 'the tracks are stacked').toBe(true);
		expect((await trackNamed(page, 'track1')).kind).toBe('curve');

		// A track's button hides it; the bar lists what is hidden, and a pick there shows it again.
		await keys.getByTestId('track-hide').click();
		await expect(keys).toHaveCount(0);
		await expect(panel.getByTestId('timeline-hidden')).toContainText('1 hidden');
		await panel.getByTestId('timeline-hidden').click();
		await menuItem('track0').click();
		await expect(keys).toBeVisible();
		await expect(panel.getByTestId('timeline-hidden')).toHaveCount(0);

		// A right click on a track opens its menu: the four ways to hide, and the delete.
		await fade.locator('header').click({ button: 'right', position: { x: 4, y: 4 } });
		for (const label of ['Hide Track', 'Hide Tracks Above', 'Hide Tracks Below', 'Hide Other Tracks', 'Delete Track…']) await expect(menuItem(label)).toBeVisible();
		await menuItem('Hide Other Tracks').click();
		await expect(fade).toBeVisible();
		await expect(keys).toHaveCount(0);
		await expect(bass).toHaveCount(0);
		await expect(panel.getByTestId('timeline-hidden')).toContainText('2 hidden');

		// What is hidden is this panel's: a second timeline panel shows every track.
		await splitRight(page);
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[1].panelId, 'timeline');
		});
		const second = page.getByTestId('timeline-panel').nth(1);
		await expect(second.getByTestId('track-track0')).toBeVisible();
		await expect(second.getByTestId('track-track2')).toBeVisible();
		await expect(panel.getByTestId('track-track0')).toHaveCount(0);
		await second.getByTestId('track-track2').locator('header').click({ button: 'right', position: { x: 4, y: 4 } });
		await menuItem('Hide Tracks Above').click();
		await expect(second.getByTestId('track-track2')).toBeVisible();
		await expect(second.getByTestId('track-track1')).toHaveCount(0);
		await expect(fade, 'the first panel still shows its one track').toBeVisible();
		await closeSplit(page);

		// A delete asks first: a cancel keeps the track, the answer removes it from the patch.
		await fade.locator('header').click({ button: 'right', position: { x: 4, y: 4 } });
		await menuItem('Delete Track…').click();
		const dialog = page.getByTestId('track-delete-dialog');
		await expect(dialog).toContainText('Delete track track1?');
		await dialog.getByRole('button', { name: 'Cancel' }).click();
		expect(await trackNamed(page, 'track1')).toBeTruthy();
		await fade.locator('header').click({ button: 'right', position: { x: 4, y: 4 } });
		await menuItem('Delete Track…').click();
		await dialog.getByTestId('track-delete-confirm').click();
		await expect.poll(() => trackNamed(page, 'track1')).toBeUndefined();
		await panel.getByTestId('timeline-hidden').click();
		await menuItem('track0').click();

		// A double click on a note lane makes a note; a drag moves it in time; its end stretches
		// it; picked, its velocity is in the track's bar, and the ✕ there removes it.
		const lane = keys.getByTestId('track-lane');
		const ground = (await lane.boundingBox())!;
		await lane.dblclick({ position: { x: ground.width * 0.25, y: ground.height * 0.5 } });
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(1);
		const born = (await trackNamed(page, 'track0')).notes[0];
		expect(born.at).toBeGreaterThan(0);
		const note = lane.locator('.note');
		await dragBy(page, note, ground.width * 0.2, 0);
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].at).toBeGreaterThan(born.at);
		expect((await trackNamed(page, 'track0')).notes[0].pitch, 'a level drag keeps the pitch').toBe(born.pitch);
		await dragBy(page, note.locator('.end'), ground.width * 0.1, 0);
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].length).toBeGreaterThan(born.length);
		const velocity = keys.getByLabel('Velocity');
		await velocity.fill('0.5');
		await velocity.press('Enter');
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].velocity).toBe(0.5);
		await keys.getByTestId('track-delete-picked').click();
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(0);

		// A curve track takes attributes; a double click on an attribute's lane makes a keyframe,
		// and a drag moves it in time and in value.
		await panel.getByTestId('timeline-add').click();
		await menuItem('Curve Track').click();
		const curve = panel.getByTestId('track-track1');
		await curve.getByTestId('track-add-attribute').click();
		const attr = curve.getByTestId('track-lane-attr0');
		const box = (await attr.boundingBox())!;
		const keysOf = async (): Promise<any[]> => Object.values<any>((await trackNamed(page, 'track1')).attributes)[0]?.keys ?? [];
		await attr.dblclick({ position: { x: box.width * 0.2, y: box.height * 0.75 } });
		await expect.poll(async () => (await keysOf()).length).toBe(1);
		await attr.dblclick({ position: { x: box.width * 0.6, y: box.height * 0.25 } });
		await expect.poll(async () => (await keysOf()).length).toBe(2);
		const [first, last] = await keysOf();
		expect(first.at < last.at && first.value < last.value, 'the keyframes are where they were made').toBe(true);
		await dragBy(page, attr.locator('.key').nth(1), box.width * 0.1, box.height * 0.2);
		await expect.poll(async () => (await keysOf())[1].value).toBeLessThan(last.value);
		expect((await keysOf())[1].at).toBeGreaterThan(last.at);
		await expectIntact(page, 'the timeline panel with a note track and a curve track');

		// The bar steers the clock: a pause, a stop at 0, a position typed, a click on the ruler,
		// the tempo, and the length the position restarts at while the loop is on.
		await panel.getByTestId('clock-play').click();
		await expect(panel.getByTestId('clock-play')).toHaveAccessibleName('Play');
		await panel.getByTestId('clock-stop').click();
		await expect(panel.getByTestId('clock-time')).toHaveValue('0');
		const time = panel.getByTestId('clock-time');
		await time.fill('3');
		await time.press('Enter');
		await expect.poll(() => clock(page, 'time')).toBe(3);
		const ruler = panel.getByTestId('timeline-ruler').locator('.lane');
		await ruler.click({ position: { x: (await ruler.boundingBox())!.width / 2, y: 4 } });
		// The span is 8 seconds, and a click lands on a whole pixel.
		await expect.poll(() => clock(page, 'time')).toBeCloseTo(4, 1);
		const bpm = panel.getByTestId('clock-bpm');
		await bpm.fill('90');
		await bpm.press('Enter');
		await expect.poll(() => clock(page, 'bpm')).toBe(90);
		const length = panel.getByTestId('clock-length');
		await length.fill('2');
		await length.press('Enter');
		await panel.getByTestId('clock-loop').click();
		await expect.poll(() => clock(page, 'loop')).toBe(1);
		await panel.getByTestId('clock-play').click();
		await expect(panel.getByTestId('clock-play')).toHaveAccessibleName('Pause');
		await expect.poll(() => clock(page, 'time'), 'the position restarts at the length').toBeLessThan(2);
	} finally {
		// The clock and the tracks are the running patch's: the next spec on this worker starts clean.
		await closeSplit(page);
		for (const [name, value] of [['loop', 0], ['length', 0], ['bpm', 120]] as const) await rawCall(page, 'variable entry edit', { name: `clock.${name}`, value });
		await rawCall(page, 'clock set', { playing: true });
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await restorePanelType(page);
	}
});

test('a held finger opens a track\'s menu, and a double tap makes a note', async ({ page, hasTouch }) => {
	test.skip(!hasTouch, 'the finger walk');
	await page.goto('/');
	await waitForApp(page);
	try {
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'timeline');
		});
		await rawCall(page, 'timeline track add', { kind: 'note', name: 'keys' });
		const track = page.getByTestId('track-keys');
		const lane = (await track.getByTestId('track-lane').boundingBox())!;
		for (let tap = 0; tap < 2; tap++) await page.touchscreen.tap(lane.x + lane.width * 0.3, lane.y + lane.height / 2);
		await expect.poll(async () => (await trackNamed(page, 'keys')).notes.length).toBe(1);
		const head = (await track.locator('header').boundingBox())!;
		const finger = await touchSession(page);
		await finger.down({ x: head.x + head.width * 0.6, y: head.y + head.height / 2 });
		await expect(page.locator('.context-menu .item', { hasText: 'Hide Other Tracks' })).toBeVisible();
		await finger.up();
		await page.keyboard.press('Escape');
		await expectIntact(page, 'the timeline panel on a phone');
	} finally {
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await restorePanelType(page);
	}
});
