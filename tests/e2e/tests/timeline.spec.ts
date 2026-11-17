// The timeline panel: tracks stacked on one time axis, hidden per panel and shown again from the
// bar's list, a track's menu with its five entries, a delete that asks first, a note and a keyframe
// made by a click and a double click and moved by a drag, the clock steered from the bar and looped in a
// section dragged on the ruler, the view zoomed by its range bar, a track resized, and the whole
// stood on its side in a panel taller than wide. On a phone, a
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
		// The walk begins with the clock at rest at 0, where the view begins.
		await rawCall(page, 'clock set', { playing: false, time: 0 });
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'timeline');
		});
		const panel = page.getByTestId('timeline-panel').first();
		await expect(panel).toContainText('No tracks');

		// The + under the words adds the first track, the bar's + the others, of either kind; the
		// tracks stack in the order they were made.
		await expect(panel.getByTestId('timeline-unit')).toContainText(/secs/i);
		await expect(panel.getByLabel('bpm')).toBeVisible();
		await panel.getByTestId('timeline-add-first').click();
		await menuItem('Note Track').click();
		for (const kind of ['Curve Track', 'Note Track']) {
			await panel.getByTestId('timeline-add').click();
			await menuItem(kind).click();
		}
		const [keys, fade, bass] = ['track0', 'track1', 'track2'].map((n) => panel.getByTestId(`track-${n}`));
		await expect(bass).toBeVisible();
		const tops = await Promise.all([keys, fade, bass].map(async (t) => (await t.boundingBox())!.y));
		expect(tops[0] < tops[1] && tops[1] < tops[2], 'the tracks are stacked').toBe(true);
		expect((await trackNamed(page, 'track1')).kind).toBe('curve');
		const shade = (t: Locator): Promise<string> => t.evaluate((el) => getComputedStyle(el).backgroundColor);
		expect(await shade(keys), 'one track is a lighter grey, the next a darker').not.toBe(await shade(fade));
		expect(await shade(keys)).toBe(await shade(bass));

		// A track's button hides it; the bar lists what is hidden, and a pick there shows it again.
		await keys.getByTestId('track-hide').click();
		await expect(keys).toHaveCount(0);
		await expect(panel.getByTestId('timeline-hidden')).toContainText('1 hidden');
		await panel.getByTestId('timeline-hidden').click();
		await menuItem('track0').click();
		await expect(keys).toBeVisible();
		await expect(panel.getByTestId('timeline-hidden')).toHaveCount(0);

		// A right click on a track opens its menu: the four ways to hide, and the delete.
		await fade.locator('.info').click({ button: 'right', position: { x: 4, y: 4 } });
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
		await second.getByTestId('track-track2').locator('.info').click({ button: 'right', position: { x: 4, y: 4 } });
		await menuItem('Hide Tracks Above').click();
		await expect(second.getByTestId('track-track2')).toBeVisible();
		await expect(second.getByTestId('track-track1')).toHaveCount(0);
		await expect(fade, 'the first panel still shows its one track').toBeVisible();
		await closeSplit(page);

		// A delete asks first: a cancel keeps the track, the answer removes it from the patch.
		await fade.locator('.info').click({ button: 'right', position: { x: 4, y: 4 } });
		await menuItem('Delete Track…').click();
		const dialog = page.getByTestId('track-delete-dialog');
		await expect(dialog).toContainText('Delete track track1?');
		await dialog.getByRole('button', { name: 'Cancel' }).click();
		await expect(dialog).toBeHidden();
		expect(await trackNamed(page, 'track1')).toBeTruthy();
		await fade.locator('.info').click({ button: 'right', position: { x: 4, y: 4 } });
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
		// The keyboard beside a note lane has a key for every pitch of its range; a scale and a
		// root dim the keys outside them, and a note made on the lane lands on the scale.
		const board = keys.getByTestId('track-keyboard').locator('span');
		await expect(board).toHaveCount(25);
		await keys.getByLabel('Highest').fill('60');
		await keys.getByLabel('Highest').press('Enter');
		await expect(board).toHaveCount(13);
		await keys.locator('[aria-label="Scale"] select').selectOption('pentatonic_major');
		await expect(keys.locator('.keys .out')).toHaveCount(7);
		await keys.locator('[aria-label="Root"] select').selectOption('D');
		await expect(keys.locator('.keys span:not(.out)').first()).toHaveAttribute('data-pitch', '50');
		expect(await trackNamed(page, 'track0')).toMatchObject({ range: [48, 60], scale: 'pentatonic_major', root: 'D' });
		await keys.locator('[aria-label="Root"] select').selectOption('C');
		await keys.getByLabel('Highest').fill('72');
		await keys.getByLabel('Highest').press('Enter');
		await expect(board).toHaveCount(25);
		const lane = keys.getByTestId('track-lane');
		const ground = (await lane.boundingBox())!;
		// The lane is cells: the one under the pointer is lit, and one click on it makes a note.
		await lane.hover({ position: { x: ground.width * 0.25, y: ground.height * 0.5 } });
		await expect(lane.getByTestId('track-cell')).toBeVisible();
		await lane.click({ position: { x: ground.width * 0.25, y: ground.height * 0.5 } });
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(1);
		const born = (await trackNamed(page, 'track0')).notes[0];
		expect(born.at).toBeGreaterThan(0);
		expect([0, 2, 4, 7, 9], 'the note is one of C pentatonic').toContain(born.pitch % 12);
		const note = lane.locator('.note');
		await dragBy(page, note, ground.width * 0.2, 0);
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].at).toBeGreaterThan(born.at);
		expect((await trackNamed(page, 'track0')).notes[0].pitch, 'a level drag keeps the pitch').toBe(born.pitch);
		// A note's end dragged makes it longer, by whole cells.
		await dragBy(page, note.locator('.end'), ground.width * 0.1, 0);
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].length).toBeGreaterThan(born.length);
		for (const [field, value] of [['Length', 2], ['Velocity', 0.5]] as const) {
			await keys.getByLabel(field).fill(String(value));
			await keys.getByLabel(field).press('Enter');
			await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0][field.toLowerCase()]).toBe(value);
		}
		// A press on another note picks it: the field that showed the first hands it nothing.
		await rawCall(page, 'timeline track edit', { track: 'track0', notes: [{ at: 1, length: 1, pitch: 60, velocity: 0.3 }, { at: 3, length: 1, pitch: 64, velocity: 0.9 }] });
		await expect(note).toHaveCount(2);
		// A velocity typed and not entered lands on its note at the next press, and the drag that
		// press begins keeps it: a note is drawn as strong as its velocity.
		await keys.getByLabel('Velocity').fill('0.6');
		const grab = (await note.nth(0).boundingBox())!;
		await page.mouse.move(grab.x + grab.width / 2, grab.y + grab.height / 2);
		await page.mouse.down();
		await expect(note.nth(0)).toHaveCSS('opacity', '0.76');
		await page.mouse.move(grab.x + grab.width / 2 + ground.width * 0.05, grab.y + grab.height / 2, { steps: 6 });
		await page.mouse.up();
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].at).toBeGreaterThan(1);
		expect((await trackNamed(page, 'track0')).notes[0].velocity).toBe(0.6);
		await keys.getByLabel('Velocity').click();
		await dragBy(page, note.nth(1), ground.width * 0.05, 0);
		await expect(keys.getByLabel('Velocity')).toHaveValue('0.9');
		expect((await trackNamed(page, 'track0')).notes.map((n: any) => n.velocity)).toEqual([0.6, 0.9]);
		await keys.getByTestId('track-delete-picked').click();
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(1);
		// One click on a note takes it out; one on an empty cell puts a note on the line of the
		// grid before it, and the grid is the bar's to choose.
		await note.first().click();
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(0);
		await panel.getByTestId('timeline-grid').locator('select').selectOption('1 s');
		await lane.click({ position: { x: ground.width * 0.3, y: ground.height * 0.5 } });
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(1);
		expect((await trackNamed(page, 'track0')).notes[0]).toMatchObject({ at: 2, length: 1 });
		// A name typed over the track's is a rename: the track is the same one.
		await keys.getByLabel('Track name').fill('lead');
		await keys.getByLabel('Track name').press('Enter');
		await expect.poll(async () => (await trackNamed(page, 'lead'))?.notes.length).toBe(1);

		// A curve track is one line: a click on its lane makes a keyframe, and a drag moves it in
		// time and in value.
		const curve = fade;
		const attr = curve.getByTestId('track-lane');
		const box = (await attr.boundingBox())!;
		const keysOf = async (): Promise<any[]> => (await trackNamed(page, 'track1')).keys;
		await attr.click({ position: { x: box.width * 0.2, y: box.height * 0.75 } });
		await expect.poll(async () => (await keysOf()).length).toBe(1);
		await attr.click({ position: { x: box.width * 0.6, y: box.height * 0.25 } });
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
		// A keyframe held near the end of the lane stays under the pointer.
		await page.mouse.move(dot.x + dot.width / 2, dot.y + dot.height / 2);
		await page.mouse.down();
		await page.mouse.move(box.x + box.width * 0.9, dot.y + dot.height / 2, { steps: 12 });
		for (let wiggle = 0; wiggle < 20; wiggle++) await page.mouse.move(box.x + box.width * 0.9 + (wiggle % 2), dot.y + dot.height / 2);
		const off = async (): Promise<number> => {
			const b = (await attr.locator('.key').nth(1).boundingBox())!;
			return Math.abs(b.x + b.width / 2 - (box.x + box.width * 0.9));
		};
		await expect.poll(off, 'the keyframe is drawn under the pointer while it is held').toBeLessThan(4);
		await page.mouse.up();
		await expect.poll(async () => (await keysOf())[1].at).toBeGreaterThan(held.at);
		expect((await keysOf())[1].at, 'nine tenths of an axis of 8 seconds').toBeCloseTo(7.2, 0);
		await expectIntact(page, 'the timeline panel with a note track and a curve track');

		// The bar steers the clock: a pause, a stop at 0, a position typed, a click on the ruler
		// and the tempo.
		await rawCall(page, 'clock set', { playing: true });
		await expect(panel.getByTestId('clock-play')).toHaveAccessibleName('Pause');
		await panel.getByTestId('clock-play').click();
		await expect(panel.getByTestId('clock-play')).toHaveAccessibleName('Play');
		await panel.getByTestId('clock-stop').click();
		await expect(panel.getByTestId('clock-time')).toHaveValue('0');
		const time = panel.getByTestId('clock-time');
		// The line of the position crosses the ruler and every track. Far past what is in view the
		// view stays where it is and shows no line, and the position's field shows the whole number.
		const line = panel.getByTestId('timeline-playhead');
		const [top, bottom] = [(await panel.getByTestId('timeline-ruler').boundingBox())!, (await panel.locator('section.track').last().boundingBox())!];
		const drawn = (await line.boundingBox())!;
		expect(drawn.y <= top.y + 1 && drawn.y + drawn.height >= bottom.y + bottom.height - 1, 'the line crosses the ruler and every track').toBe(true);
		const lastMark = async (): Promise<number> => Number(await panel.getByTestId('timeline-ruler').locator('.tick').last().innerText());
		const marked = await lastMark();
		await rawCall(page, 'clock set', { time: 1234.56 });
		await expect(time).toHaveValue('1234.56');
		await expect(line).toHaveCount(0);
		expect(await lastMark(), 'the view stays').toBe(marked);
		expect(await time.evaluate((el: HTMLInputElement) => el.scrollWidth <= el.clientWidth), 'the field shows the whole position').toBe(true);
		await rawCall(page, 'clock set', { time: 0 });
		await expect(time).toHaveValue('0');
		await time.fill('3');
		await time.press('Enter');
		await expect.poll(() => clock(page, 'time')).toBe(3);
		// A focus that types nothing is no edit: the clock stays where another hand put it.
		await time.click();
		const playhead = panel.getByTestId('timeline-playhead');
		const stood = (await playhead.boundingBox())!.x;
		await rawCall(page, 'clock set', { time: 5 });
		await expect.poll(async () => (await playhead.boundingBox())!.x, 'the new position reaches the page under the focus').not.toBe(stood);
		await panel.getByTestId('clock-bpm').click();
		await expect(time).toHaveValue('5');
		expect(await clock(page, 'time')).toBe(5);
		// A click half way along the ruler puts the position at the time the marks give that place.
		const ruler = panel.getByTestId('timeline-ruler').locator('.lane');
		const axis = (await ruler.boundingBox())!;
		const marks = await Promise.all([ruler.locator('.tick').first(), ruler.locator('.tick').last()].map(async (m) => ({ x: (await m.boundingBox())!.x, at: Number(await m.innerText()) })));
		const perSecond = (marks[1].x - marks[0].x) / (marks[1].at - marks[0].at);
		const timeAt = (share: number): number => marks[0].at + (axis.x + axis.width * share - marks[0].x) / perSecond;
		await ruler.click({ position: { x: axis.width / 2, y: 4 } });
		await expect.poll(() => clock(page, 'time')).toBeCloseTo(timeAt(0.5), 1);
		const bpm = panel.getByTestId('clock-bpm');
		await bpm.fill('90');
		await bpm.press('Enter');
		await expect.poll(() => clock(page, 'bpm')).toBe(90);

		// A drag along the ruler marks the section the clock loops in: the position restarts at
		// the section's start.
		const mark = async (): Promise<void> => {
			await page.mouse.move(axis.x + axis.width * 0.25, axis.y + 4);
			await page.mouse.down();
			await page.mouse.move(axis.x + axis.width * 0.5, axis.y + 4, { steps: 6 });
			await page.mouse.up();
			await expect.poll(() => clock(page, 'length')).toBeGreaterThan(0);
		};
		await mark();
		expect(await clock(page, 'length')).toBeCloseTo(timeAt(0.5) - timeAt(0.25), 1);
		const start = await clock(page, 'start');
		expect(start).toBeCloseTo(timeAt(0.25), 1);
		await expect(panel.getByTestId('timeline-loop')).toBeVisible();
		await panel.getByTestId('clock-play').click();
		let before = await clock(page, 'time');
		await expect
			.poll(async () => {
				const now = await clock(page, 'time');
				const restarted = now < before;
				before = now;
				return restarted;
			}, 'the position restarts inside the section')
			.toBe(true);
		expect(before).toBeGreaterThanOrEqual(start);
		await rawCall(page, 'clock set', { playing: false });
		// A double click on the section looks at the section alone, and keeps it; the loop chip
		// clears it, and a double click on the ruler then looks at everything again.
		const firstMark = async (): Promise<number> => Number(await ruler.locator('.tick').first().innerText());
		await ruler.dblclick({ position: { x: axis.width * 0.375, y: 4 } });
		await expect.poll(firstMark).toBeCloseTo(start, 1);
		expect(await lastMark()).toBeCloseTo(start + (await clock(page, 'length')), 1);
		await panel.getByTestId('clock-loop').click();
		await expect.poll(() => clock(page, 'length')).toBe(0);
		await expect(panel.getByTestId('timeline-loop')).toHaveCount(0);
		await ruler.dblclick({ position: { x: axis.width / 2, y: 4 } });
		await expect.poll(lastMark).toBe(marked);
		// A click on the ruler outside the section clears it.
		await mark();
		await ruler.click({ position: { x: axis.width * 0.75, y: 4 } });
		await expect.poll(() => clock(page, 'length')).toBe(0);

		// The range bar is the view: its end dragged in zooms in, and dragged out past the end of
		// the bar zooms out past everything there is.
		await rawCall(page, 'clock set', { playing: false, time: 0 });
		await expect(time).toHaveValue('0');
		await expect.poll(lastMark).toBe(marked);
		const bar = (await panel.getByTestId('range-bar').boundingBox())!;
		expect(bar.y + bar.height, 'the range bar is right above the ruler').toBeLessThanOrEqual(axis.y + 1);
		await dragBy(page, panel.getByTestId('range-to'), -bar.width / 2, 0);
		await expect.poll(lastMark).toBeLessThan(6);
		await dragBy(page, panel.getByTestId('range-to'), bar.width * 1.5, 0);
		await expect.poll(lastMark).toBeGreaterThan(12);

		// A track's edge dragged resizes the track in this panel, and changes nothing of the track.
		const tall = async (): Promise<number> => (await curve.locator('.lane').boundingBox())!.height;
		const [was, tracks] = [await tall(), JSON.stringify((await backendDoc(page)).timeline)];
		await dragBy(page, curve.getByTestId('track-grip'), 0, 40);
		await expect.poll(tall).toBeGreaterThan(was + 30);
		expect(JSON.stringify((await backendDoc(page)).timeline)).toBe(tracks);

		// In a panel taller than wide the tracks stand beside each other and the time runs down.
		const lead = panel.getByTestId('track-lead');
		const screen = page.viewportSize()!;
		await page.setViewportSize({ width: 600, height: 900 });
		await expect(panel).toHaveClass(/vertical/);
		const [left, right] = [(await lead.boundingBox())!, (await curve.boundingBox())!];
		expect(Math.abs(left.y - right.y) < 2 && left.x < right.x, 'the tracks are columns').toBe(true);
		const upright = (await lead.locator('.note').first().boundingBox())!;
		expect(upright.height, 'a note runs down its column').toBeGreaterThan(upright.width);
		await expectIntact(page, 'the timeline panel standing');
		await page.setViewportSize(screen);
		await expect(panel).not.toHaveClass(/vertical/);
	} finally {
		// The clock and the tracks are the running patch's: the next spec on this worker starts clean.
		for (const [name, value] of [['start', 0], ['length', 0], ['bpm', 120]] as const) await rawCall(page, 'variable entry edit', { name: `clock.${name}`, value });
		await rawCall(page, 'clock set', { playing: true });
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await closeSplit(page);
		await restorePanelType(page);
	}
});

test('a held finger opens a track\'s menu, and a tap makes a note and takes it out', async ({ page, hasTouch }) => {
	test.skip(!hasTouch, 'the finger walk');
	await page.goto('/');
	await waitForApp(page);
	try {
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'timeline');
		});
		await rawCall(page, 'clock set', { playing: false, time: 0 });
		await rawCall(page, 'timeline track add', { kind: 'note', name: 'keys' });
		const track = page.getByTestId('track-keys');
		const lane = (await track.getByTestId('track-lane').boundingBox())!;
		for (const notes of [1, 0]) {
			// Inside a cell, clear of the line it begins on and of the end a drag stretches.
			await page.touchscreen.tap(lane.x + lane.width * 0.3, lane.y + lane.height * 0.505);
			await expect.poll(async () => (await trackNamed(page, 'keys')).notes.length).toBe(notes);
		}
		const head = (await track.locator('.info').boundingBox())!;
		const finger = await touchSession(page);
		await finger.down({ x: head.x + head.width * 0.6, y: head.y + head.height / 2 });
		await expect(page.locator('.context-menu .item', { hasText: 'Hide Other Tracks' })).toBeVisible();
		await finger.up();
		// A held finger is also the browser's own context-menu gesture, which opens the menu again
		// when it comes late: Escape until none is open.
		await expect(async () => {
			await page.keyboard.press('Escape');
			expect(await page.locator('.context-menu').count()).toBe(0);
		}).toPass();
		// Two keyframes closer than a finger is wide: a tap on the first one's dot picks that one,
		// not the one whose hit area reaches over it.
		await rawCall(page, 'timeline track add', { kind: 'curve', name: 'fade' });
		const near = [2, 2.2].map((at) => ({ at, value: 0.5, curve: 'linear' }));
		await rawCall(page, 'timeline track edit', { track: 'fade', keys: near });
		const fade = page.getByTestId('track-fade');
		const dots = fade.locator('.key');
		const [a, b] = [(await dots.nth(0).boundingBox())!, (await dots.nth(1).boundingBox())!];
		expect(Math.hypot(b.x - a.x, b.y - a.y), 'the dots are closer than half a hit area').toBeLessThan(22);
		await page.touchscreen.tap(a.x + a.width / 2, a.y + a.height / 2);
		await expect(fade.getByLabel('Value')).toBeVisible();
		expect(await dots.nth(0).getAttribute('class'), 'the dot under the finger is the one picked').toContain('picked');
		await expectIntact(page, 'the timeline panel on a phone');
	} finally {
		await rawCall(page, 'clock set', { playing: true });
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await restorePanelType(page);
	}
});
