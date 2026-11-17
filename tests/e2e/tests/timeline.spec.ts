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
		await expect(dialog).toBeHidden();
		expect(await trackNamed(page, 'track1')).toBeTruthy();
		await fade.locator('header').click({ button: 'right', position: { x: 4, y: 4 } });
		await menuItem('Delete Track…').click();
		await dialog.getByTestId('track-delete-confirm').click();
		await expect.poll(() => trackNamed(page, 'track1')).toBeUndefined();
		// An undo brings the track back, as the dialog says.
		await rawCall(page, 'undo');
		await expect(fade).toBeVisible();
		await panel.getByTestId('timeline-hidden').click();
		await menuItem('track0').click();

		// A double click on a note lane makes a note; a drag moves it in time; picked, its length
		// and its velocity are in the track's bar, and the ✕ there removes it.
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
		for (const [field, value] of [['Length', 2], ['Velocity', 0.5]] as const) {
			await keys.getByLabel(field).fill(String(value));
			await keys.getByLabel(field).press('Enter');
			await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0][field.toLowerCase()]).toBe(value);
		}
		// A press on another note picks it: the field that showed the first hands it nothing.
		await rawCall(page, 'timeline track edit', { track: 'track0', notes: [{ at: 1, length: 1, pitch: 60, velocity: 0.3 }, { at: 3, length: 1, pitch: 64, velocity: 0.9 }] });
		await expect(note).toHaveCount(2);
		await note.nth(0).click();
		await keys.getByLabel('Velocity').click();
		await note.nth(1).click();
		await expect(keys.getByLabel('Velocity')).toHaveValue('0.9');
		expect((await trackNamed(page, 'track0')).notes.map((n: any) => n.velocity)).toEqual([0.3, 0.9]);
		await keys.getByTestId('track-delete-picked').click();
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(1);
		// A name typed over the track's is a rename: the track is the same one.
		await keys.getByLabel('Track name').fill('lead');
		await keys.getByLabel('Track name').press('Enter');
		await expect.poll(async () => (await trackNamed(page, 'lead'))?.notes.length).toBe(1);

		// A curve track takes attributes; a double click on an attribute's lane makes a keyframe,
		// and a drag moves it in time and in value.
		const curve = fade;
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
		// A press that hardly moves picks the keyframe and edits nothing: its value shows in the bar.
		const held = (await keysOf())[1];
		const dot = (await attr.locator('.key').nth(1).boundingBox())!;
		await page.mouse.move(dot.x + dot.width / 2 + 3, dot.y + dot.height / 2 + 3);
		await page.mouse.down();
		await page.mouse.move(dot.x + dot.width / 2 + 4, dot.y + dot.height / 2 + 3);
		await page.mouse.up();
		await expect(curve.getByLabel('Value')).toHaveValue(String(held.value));
		expect((await keysOf())[1]).toEqual(held);
		// A keyframe held near the end of the lane stays under the pointer: the axis does not
		// grow under its own drag.
		await page.mouse.move(dot.x + dot.width / 2, dot.y + dot.height / 2);
		await page.mouse.down();
		await page.mouse.move(box.x + box.width * 0.9, dot.y + dot.height / 2, { steps: 12 });
		for (let wiggle = 0; wiggle < 20; wiggle++) await page.mouse.move(box.x + box.width * 0.9 + (wiggle % 2), dot.y + dot.height / 2);
		await page.mouse.up();
		await expect.poll(async () => (await keysOf())[1].at).toBeGreaterThan(held.at);
		expect((await keysOf())[1].at, 'nine tenths of an axis of 8 seconds').toBeCloseTo(7.2, 0);
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
		// A focus that types nothing is no edit: the clock stays where another hand put it.
		await time.click();
		await rawCall(page, 'clock set', { time: 5 });
		await panel.getByTestId('clock-bpm').click();
		await expect(time).toHaveValue('5');
		expect(await clock(page, 'time')).toBe(5);
		// A click half way along the ruler puts the position at the time the marks give that place.
		const ruler = panel.getByTestId('timeline-ruler').locator('.lane');
		const [axis, mark] = [(await ruler.boundingBox())!, ruler.locator('.tick').last()];
		const perSecond = ((await mark.boundingBox())!.x - axis.x) / Number(await mark.innerText());
		await ruler.click({ position: { x: axis.width / 2, y: 4 } });
		await expect.poll(() => clock(page, 'time')).toBeCloseTo(axis.width / 2 / perSecond, 1);
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
		for (const [name, value] of [['loop', 0], ['length', 0], ['bpm', 120]] as const) await rawCall(page, 'variable entry edit', { name: `clock.${name}`, value });
		await rawCall(page, 'clock set', { playing: true });
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await closeSplit(page);
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
