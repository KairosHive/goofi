// The timeline panel: tracks stacked on one time axis, hidden per panel and shown again from the
// bar's list, a track's menu with its five entries, a delete that asks first, a note and a keyframe
// made by a click and moved by a drag, the clock steered from the bar and looped in a
// section dragged on the ruler, the view zoomed by its range bar, a track resized, and the whole
// stood on its side in a panel taller than wide. A track's settings stand in groups beside its
// lane, and those that do not fit are in the menu in its title; its edge held at the end of the
// track area scrolls the area and goes on. On a phone, a held finger is the door onto the menu, a
// tap makes the note, a finger held on a note or a keyframe opens its menu, and the title's menu
// holds hide and delete. What is picked has its settings in a menu on it, the Delete key takes
// it out, and a wheel moves and zooms the view. A track records its input.

import { test, expect, type Locator, type Page } from '@playwright/test';
import { closeSplit, restorePanelType, splitRight, waitForApp } from '../lib/app';
import { addNode, waitForNode } from '../lib/goofi';
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

/** Where a track's setting is: in its section beside the lane, which this unfolds where it is
 * folded, else in the menu in the track's title, which this opens. */
async function settings(page: Page, track: Locator, setting: string): Promise<Locator> {
	const [beside, menu, more] = [track.locator(setting).first(), page.getByTestId('track-more-menu'), track.getByTestId('track-more')];
	if (await beside.isVisible()) return beside;
	if (await menu.isVisible()) return menu.locator(setting);
	for (const folded of await track.locator('.groups button[aria-expanded="false"]:visible').all()) await folded.click();
	// An unfolded section stands beside the lane, where the track grows for it, or is in the menu.
	await expect.poll(async () => (await beside.isVisible()) || (await more.isVisible())).toBe(true);
	if (await beside.isVisible()) return beside;
	await more.click();
	return menu.locator(setting);
}
/** Close the track's menu, if one is open: an editor in it may take the first Escape for itself. */
async function shut(page: Page): Promise<void> {
	await expect(async () => {
		await page.keyboard.press('Escape');
		expect(await page.getByTestId('track-more-menu').count()).toBe(0);
	}).toPass();
}
/** Pick an option of the track's dropdown named `name`, and close the menu it may be in. */
async function choose(page: Page, track: Locator, name: string, option: string): Promise<void> {
	await (await settings(page, track, `select[aria-label="${name}"]`)).selectOption(option);
	await shut(page);
}

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
		// The walk begins with the clock at rest at 0, where the view begins, and at a tempo of 60:
		// the timeline counts beats, and a beat is then a second.
		await rawCall(page, 'clock set', { playing: false, time: 0 });
		await rawCall(page, 'variable entry edit', { name: 'clock.bpm', value: 60 });
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'timeline');
		});
		const panel = page.getByTestId('timeline-panel').first();
		await expect(panel).toContainText('No tracks');

		// The + under the words adds the first track, the bar's + the others, of either kind; the
		// tracks stack in the order they were made.
		await expect(panel.getByLabel('dur')).toBeVisible();
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

		// A track dragged by what it says about itself, onto another, takes that one's place.
		const names = async (): Promise<string[]> => panel.locator('section.track').evaluateAll((all) => all.map((t) => t.querySelector<HTMLInputElement>('.name input, input.name')?.value ?? ''));
		expect(await names()).toEqual(['track0', 'track1', 'track2']);
		const [from, onto] = [(await keys.locator('.info').boundingBox())!, (await bass.locator('.info').boundingBox())!];
		// By the column's own edge: the rest of it is fields and buttons, which a press goes into.
		await page.mouse.move(from.x + 1.5, from.y + from.height / 2);
		await page.mouse.down();
		await page.mouse.move(onto.x + 20, onto.y + onto.height / 2, { steps: 8 });
		await page.mouse.up();
		await expect.poll(names).toEqual(['track1', 'track2', 'track0']);
		await rawCall(page, 'timeline track move', { track: 'track0', before: 'track1' });
		await expect.poll(names).toEqual(['track0', 'track1', 'track2']);

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
		await fade.getByTestId('track-delete').click();
		await dialog.getByTestId('track-delete-confirm').click();
		await expect.poll(() => trackNamed(page, 'track1')).toBeUndefined();
		// An undo brings the track back, as the dialog says.
		await rawCall(page, 'undo');
		await expect(fade).toBeVisible();
		await panel.getByTestId('timeline-hidden').click();
		await menuItem('track0').click();

		// A click on a note lane makes a note; a drag moves it in time; picked, its length
		// and its velocity are in the track's bar, and the ✕ there removes it.
		// The keyboard beside a note lane has a key for every pitch of its range; a scale and a
		// root dim the keys outside them, and a note made on the lane lands on the scale.
		const board = keys.getByTestId('track-keyboard').locator('span');
		await expect(board).toHaveCount(25);
		// The track's settings stand in named sections beside its lane, each one folds, and the
		// recording's begins folded with its button on its own line. A pitch is picked by its name.
		const sectionOpen = (name: string): Locator => keys.getByTestId(`track-section-${name}`).locator('button[aria-expanded]');
		await expect(sectionOpen('record')).toHaveAttribute('aria-expanded', 'false');
		await expect(keys.getByTestId('track-section-record').getByTestId('track-record')).toBeVisible();
		await expect(sectionOpen('keyboard')).toHaveAttribute('aria-expanded', 'true');
		await sectionOpen('keyboard').click();
		await expect(keys.locator('select[aria-label="Scale"]')).toHaveCount(0);
		await sectionOpen('keyboard').click();
		await expect(keys.locator('select[aria-label="Scale"] option:checked')).toHaveText('chromatic');
		// A pointer on a section's name lights the ground under all the name folds, the name's
		// line and the settings under it, from one edge of what the track says to the bar of the
		// keyboard's range.
		const lit = (): Promise<{ ground: string; size: number[] }> =>
			sectionOpen('keyboard').evaluate((el) => {
				const under = getComputedStyle(el, '::after');
				return { ground: under.backgroundColor, size: [under.width, under.height].map((v) => Math.round(parseFloat(v))) };
			});
		const [whole, said, piano, ranger] = [(await keys.getByTestId('track-section-keyboard').boundingBox())!, (await keys.locator('.info').boundingBox())!, (await keys.getByTestId('track-keyboard').boundingBox())!, (await keys.getByTestId('track-range-bar').boundingBox())!];
		await page.mouse.move(piano.x + piano.width + 40, piano.y + 4);
		await expect.poll(async () => (await lit()).ground, 'no ground at rest').toBe('rgba(0, 0, 0, 0)');
		await sectionOpen('keyboard').hover();
		await expect.poll(async () => (await lit()).ground).not.toBe('rgba(0, 0, 0, 0)');
		expect((await lit()).size, 'the lit ground is the whole section').toEqual([Math.round(whole.width), Math.round(whole.height)]);
		expect([Math.round(whole.x), Math.round(whole.x + whole.width)], 'the section goes from the edge to the range bar').toEqual([Math.round(said.x), Math.round(ranger.x)]);
		// The keys the keyboard shows are a bar between what the track says and the keyboard, as
		// long as every key there is, the highest at its top: an end dragged changes that end by
		// whole keys, a drag of the bar moves both, and each drag is one step of the history.
		expect(Math.round(ranger.x + ranger.width), 'the bar stands against the keyboard').toBe(Math.round(piano.x));
		const keyRange = async (): Promise<number[]> => (await trackNamed(page, 'track0')).range;
		const octave = (ranger.height * 12) / 128;
		await dragBy(page, keys.getByTestId('track-range-from'), 0, octave);
		await expect.poll(keyRange).toEqual([48, 60]);
		await expect(board).toHaveCount(13);
		await dragBy(page, keys.getByTestId('track-range-bar').locator('.thumb'), 0, octave);
		await expect.poll(keyRange).toEqual([36, 48]);
		await page.evaluate(() => (window as any).goofi.commands.undo());
		await expect.poll(keyRange, 'the drag is one step back').toEqual([48, 60]);
		// A drag past the last key stops there, as long as it was.
		await dragBy(page, keys.getByTestId('track-range-bar').locator('.thumb'), 0, ranger.height);
		await expect.poll(keyRange).toEqual([0, 12]);
		await page.evaluate(() => (window as any).goofi.commands.undo());
		await expect.poll(keyRange).toEqual([48, 60]);
		await choose(page, keys, 'Scale', 'pentatonic_major');
		await expect(keys.locator('.keys .out')).toHaveCount(7);
		await choose(page, keys, 'Root', 'D');
		await expect(keys.locator('.keys span:not(.out)').first()).toHaveAttribute('data-pitch', '50');
		expect(await trackNamed(page, 'track0')).toMatchObject({ range: [48, 60], scale: 'pentatonic_major', root: 'D' });
		await choose(page, keys, 'Root', 'C');
		await dragBy(page, keys.getByTestId('track-range-from'), 0, -octave);
		await expect.poll(keyRange).toEqual([48, 72]);
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
		// A drag across the lane that drifts less than half a cell along it changes the pitch,
		// and leaves the note on the line it had.
		const level = (await trackNamed(page, 'track0')).notes[0];
		await dragBy(page, note, ground.width * 0.004, -ground.height * 0.2);
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].pitch).toBeGreaterThan(level.pitch);
		expect((await trackNamed(page, 'track0')).notes[0].at, 'the drift is no move in time').toBe(level.at);
		// A note's end dragged makes it longer, by whole cells.
		await dragBy(page, note.locator('.end'), ground.width * 0.1, 0);
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].length).toBeGreaterThan(born.length);
		// With Shift body a drag leaves the grid: the note goes where the pointer is.
		const body = (await note.boundingBox())!;
		await page.mouse.move(body.x + 10, body.y + body.height / 2);
		await page.mouse.down();
		await page.keyboard.down('Shift');
		await page.mouse.move(body.x + 10 + ground.width * 0.01, body.y + body.height / 2, { steps: 4 });
		await page.mouse.up();
		await page.keyboard.up('Shift');
		await expect.poll(async () => ((await trackNamed(page, 'track0')).notes[0].at * 4) % 1).not.toBe(0);
		// A picked note's settings are in a menu on the note, which a right click opens, and not
		// among the track's own: its pitch, its length and its velocity. The track's menu stays shut.
		const pickedMenu = page.getByTestId('picked-menu');
		await expect(keys.getByTestId('track-section-item')).toHaveCount(0);
		await note.click({ button: 'right' });
		await expect(pickedMenu).toBeVisible();
		await expect(page.locator('.context-menu')).toHaveCount(0);
		expect((await trackNamed(page, 'track0')).notes.length, 'the right press made no note').toBe(1);
		for (const [field, value] of [['Length', 2], ['Velocity', 0.5]] as const) {
			await pickedMenu.getByLabel(field).fill(String(value));
			await pickedMenu.getByLabel(field).press('Enter');
			await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0][field.toLowerCase()]).toBe(value);
		}
		await pickedMenu.locator('select[aria-label="Pitch"]').selectOption('62');
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes[0].pitch).toBe(62);
		// A press beside the menu closes it and does nothing else: the lane gets no note.
		await page.mouse.click(ground.x + ground.width * 0.9, ground.y + ground.height * 0.1);
		await expect(pickedMenu).toHaveCount(0);
		expect((await trackNamed(page, 'track0')).notes.length).toBe(1);
		// A velocity typed and not entered lands on its note when the menu closes: a note is drawn
		// as strong as its velocity. The note the menu was on is the one picked, in an outline of
		// the accent over a fill of the accent held back, and the Delete key takes out what is picked.
		await rawCall(page, 'timeline track edit', { track: 'track0', notes: [{ at: 1, length: 1, pitch: 60, velocity: 0.3 }, { at: 3, length: 1, pitch: 64, velocity: 0.9 }] });
		await expect(note).toHaveCount(2);
		await note.nth(0).click({ button: 'right' });
		await pickedMenu.getByLabel('Velocity').fill('0.6');
		await page.mouse.click(ground.x + ground.width * 0.9, ground.y + ground.height * 0.1);
		await expect(pickedMenu).toHaveCount(0);
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.map((n: any) => n.velocity)).toEqual([0.6, 0.9]);
		// The velocity is in the note's fill alone: the line around a picked note is as strong on
		// a soft note as on a loud one.
		await expect.poll(() => note.nth(0).evaluate((el) => getComputedStyle(el).getPropertyValue('--v'))).toBe('0.76');
		await expect(lane.locator('.note.picked')).toHaveCount(1);
		await expect(lane.locator('.note.picked')).toHaveCSS('outline-color', 'rgb(80, 208, 160)');
		expect(await lane.locator('.note.picked').evaluate((el) => getComputedStyle(el).backgroundColor), 'its fill is the accent held back').toMatch(/0\.4\)$/);
		await expect(lane.locator('.note.picked')).toHaveCSS('opacity', '1');
		await page.keyboard.press('Delete');
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.map((n: any) => n.pitch)).toEqual([64]);
		// A click on a note with Ctrl held picks it, and one more unpicks it; neither takes it out.
		// A click with no key takes the note out. One click on an empty cell puts a note on the
		// line of the grid before it, picked by nothing, and the grid is the track's to choose.
		await expect(lane.locator('.note.picked')).toHaveCount(0);
		await note.first().click({ modifiers: ['ControlOrMeta'] });
		await expect(lane.locator('.note.picked')).toHaveCount(1);
		await note.first().click({ modifiers: ['ControlOrMeta'] });
		await expect(lane.locator('.note.picked')).toHaveCount(0);
		expect((await trackNamed(page, 'track0')).notes.length).toBe(1);
		await note.first().click();
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(0);
		await choose(page, keys, 'Grid', '1 s');
		await expect.poll(async () => (await trackNamed(page, 'track0')).grid).toBe('1 s');
		expect((await trackNamed(page, 'track2')).grid, 'another note track keeps its own grid').toBe('1/4 beat');
		await lane.click({ position: { x: ground.width * 0.3, y: ground.height * 0.5 } });
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(1);
		expect((await trackNamed(page, 'track0')).notes[0]).toMatchObject({ at: 2, length: 1 });
		await expect(lane.locator('.note.picked'), 'a note made is not a note picked').toHaveCount(0);
		// A drag along the ground makes a note in each cell it goes through, as one edit.
		await page.mouse.move(ground.x + ground.width * 0.5, ground.y + ground.height * 0.25);
		await page.mouse.down();
		await page.mouse.move(ground.x + ground.width * 0.9, ground.y + ground.height * 0.25, { steps: 12 });
		await expect.poll(async () => lane.locator('.note').count(), 'the notes show as they are made').toBeGreaterThan(2);
		await page.mouse.up();
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBeGreaterThan(2);
		const row = (await trackNamed(page, 'track0')).notes.slice(1);
		expect(new Set(row.map((n: any) => n.pitch)).size === 1 && new Set(row.map((n: any) => n.at)).size === row.length, 'one note a cell, along one pitch').toBe(true);
		await page.evaluate(() => (window as any).goofi.commands.undo());
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length, 'the drag is one step back').toBe(1);
		// A click with Shift held picks through: every note in the box from the note picked last
		// to the one clicked, and no note outside it. With no note picked it is a click with Ctrl.
		// Escape picks none.
		await rawCall(page, 'timeline track edit', { track: 'track0', notes: [[1, 60], [2, 62], [3, 64], [5, 70], [2, 67]].map(([at, pitch]) => ({ at, length: 1, pitch, velocity: 0.8 })) });
		await expect(note).toHaveCount(5);
		const pickedNotes = (): Promise<number[]> => note.evaluateAll((all) => all.flatMap((el, k) => (el.classList.contains('picked') ? [k] : [])));
		await note.nth(0).click({ modifiers: ['Shift'] });
		expect(await pickedNotes(), 'with none picked, Shift picks the one clicked').toEqual([0]);
		await note.nth(2).click({ modifiers: ['Shift'] });
		expect(await pickedNotes(), 'the box from the first to the third holds the second, and not the one above it').toEqual([0, 1, 2]);
		await note.nth(3).click({ modifiers: ['ControlOrMeta'] });
		expect(await pickedNotes()).toEqual([0, 1, 2, 3]);
		expect((await trackNamed(page, 'track0')).notes.length, 'no click with a key held took a note out').toBe(5);
		await page.keyboard.press('Escape');
		expect(await pickedNotes()).toEqual([]);
		// A drag with Shift held draws a box that picks every note it touches: they move as one,
		// a field of the menu on one of them sets them all, and the Delete key takes them all out.
		const ats = async (): Promise<number[]> => (await trackNamed(page, 'track0')).notes.map((n: any) => n.at);
		await rawCall(page, 'timeline track edit', { track: 'track0', notes: [{ at: 2, length: 1, pitch: 60, velocity: 0.8 }, { at: 4, length: 1, pitch: 64, velocity: 0.8 }] });
		await expect(note).toHaveCount(2);
		await page.keyboard.down('Shift');
		await page.mouse.move(ground.x + ground.width * 0.1, ground.y + 2);
		await page.mouse.down();
		await page.mouse.move(ground.x + ground.width * 0.8, ground.y + ground.height - 2, { steps: 6 });
		await expect(panel.getByTestId('timeline-box')).toBeVisible();
		await page.mouse.up();
		await page.keyboard.up('Shift');
		await expect(lane.locator('.note.picked')).toHaveCount(2);
		await dragBy(page, note.nth(0), ground.width * 0.125, 0);
		await expect.poll(ats).toEqual([3, 5]);
		await note.nth(1).click({ button: 'right' });
		await expect(lane.locator('.note.picked'), 'a right click on a picked note keeps what is picked').toHaveCount(2);
		await expect(pickedMenu, 'a note\'s menu has no way to take it out: a click does that').not.toContainText('Delete');
		await pickedMenu.getByLabel('Velocity').fill('0.4');
		await pickedMenu.getByLabel('Velocity').press('Enter');
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.map((n: any) => n.velocity)).toEqual([0.4, 0.4]);
		await page.keyboard.press('Escape');
		await expect(pickedMenu).toHaveCount(0);
		await page.keyboard.press('Delete');
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(0);
		await page.evaluate(() => (window as any).goofi.commands.undo());
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length, 'the delete is one step back').toBe(2);
		await page.evaluate(() => (window as any).goofi.commands.redo());
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(0);
		await lane.click({ position: { x: ground.width * 0.3, y: ground.height * 0.5 } });
		await expect.poll(async () => (await trackNamed(page, 'track0')).notes.length).toBe(1);
		// A name typed over the track's is a rename: the track is the same one.
		await keys.getByLabel('Track name').fill('lead');
		await keys.getByLabel('Track name').press('Enter');
		await expect.poll(async () => (await trackNamed(page, 'lead'))?.notes.length).toBe(1);

		// A curve track is one line: a click on its lane makes a keyframe, and a drag moves it in
		// time and in value.
		const curve = fade;
		// What a curve's lane shows is between two limits, at the two ends of the bar beside the
		// lane: a click on one makes it a field, and a drag of the bar moves both by as much.
		const limits = async (): Promise<number[]> => (await trackNamed(page, 'track1')).range;
		await expect(curve.getByTestId('track-range-most')).toHaveText('1');
		await expect(curve.getByTestId('track-range-least')).toHaveText('0');
		// The bar is where a keyboard's is, as long as it, and each limit is beside its end of the
		// bar: the most at its top, the least at its bottom.
		const [rail, lined] = [(await curve.getByTestId('track-range-bar').boundingBox())!, (await panel.getByTestId('track-lead').getByTestId('track-range-bar').boundingBox())!];
		expect([Math.round(rail.x), Math.round(rail.width)], 'the bars of the two kinds of track are in one line').toEqual([Math.round(lined.x), Math.round(lined.width)]);
		const [most, least] = [(await curve.getByTestId('track-range-most').boundingBox())!, (await curve.getByTestId('track-range-least').boundingBox())!];
		expect(most.x >= rail.x + rail.width - 1 && least.x >= rail.x + rail.width - 1, 'the limits are beside the bar').toBe(true);
		expect([Math.abs(most.y - rail.y) <= 1, Math.abs(least.y + least.height - (rail.y + rail.height)) <= 1], 'each at its end of the bar').toEqual([true, true]);
		await curve.getByTestId('track-range-most').click();
		await curve.getByTestId('track-range-most-input').fill('2');
		await curve.getByTestId('track-range-most-input').press('Enter');
		await expect.poll(limits).toEqual([0, 2]);
		await dragBy(page, curve.getByTestId('track-range-bar').locator('.thumb'), 0, 12);
		await expect.poll(async () => (await limits())[0]).toBeLessThan(0);
		expect((await limits())[1] - (await limits())[0], 'the range is as long as it was').toBeCloseTo(2, 2);
		await rawCall(page, 'timeline track edit', { track: 'track1', range: [0, 1] });
		await expect.poll(limits).toEqual([0, 1]);
		const attr = curve.getByTestId('track-lane');
		const box = (await attr.boundingBox())!;
		const keysOf = async (): Promise<any[]> => (await trackNamed(page, 'track1')).keys;
		await attr.click({ position: { x: box.width * 0.2, y: box.height * 0.75 } });
		await expect.poll(async () => (await keysOf()).length).toBe(1);
		await attr.click({ position: { x: box.width * 0.6, y: box.height * 0.25 } });
		await expect.poll(async () => (await keysOf()).length).toBe(2);
		const [first, last] = await keysOf();
		expect(first.at < last.at && first.value < last.value, 'the keyframes are where they were made').toBe(true);
		// A keyframe eases as the one before it does, else as the one after; with neither, it is smooth.
		expect([first.curve, last.curve]).toEqual(['smooth', 'smooth']);
		await rawCall(page, 'timeline track edit', { track: 'track1', keys: [{ ...first, curve: 'step' }, { ...last, curve: 'easing' }] });
		await attr.click({ position: { x: box.width * 0.4, y: box.height * 0.5 } });
		await expect.poll(async () => (await keysOf()).map((k) => k.curve)).toEqual(['step', 'step', 'easing']);
		await attr.click({ position: { x: box.width * 0.05, y: box.height * 0.5 } });
		await expect.poll(async () => (await keysOf()).map((k) => k.curve)).toEqual(['step', 'step', 'step', 'easing']);
		// A drag along the lane's ground makes a keyframe in each stretch it goes through, as one edit.
		await rawCall(page, 'timeline track edit', { track: 'track1', keys: [] });
		await expect(attr.locator('.key')).toHaveCount(0);
		await page.mouse.move(box.x + box.width * 0.1, box.y + box.height * 0.8);
		await page.mouse.down();
		await page.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.2, { steps: 10 });
		await expect.poll(async () => attr.locator('.key').count(), 'the keyframes show as they are made').toBeGreaterThan(2);
		await page.mouse.up();
		await expect.poll(async () => (await keysOf()).length).toBeGreaterThan(2);
		const painted = await keysOf();
		expect(painted.every((k, i) => i === 0 || (k.at > painted[i - 1].at && k.value > painted[i - 1].value)), 'they follow the drag').toBe(true);
		await page.evaluate(() => (window as any).goofi.commands.undo());
		await expect.poll(async () => (await keysOf()).length, 'the drag is one step back').toBe(0);
		// A drag draws over what was there: a keyframe from before it, between the first and the
		// last it makes, is removed, and the keyframes outside that stretch stay.
		for (const x of [0.2, 0.5, 0.8]) await attr.click({ position: { x: box.width * x, y: box.height * 0.5 } });
		await expect.poll(async () => (await keysOf()).length).toBe(3);
		const [outer, inPath, other] = (await keysOf()).map((k) => k.at);
		await page.mouse.move(box.x + box.width * 0.4, box.y + box.height * 0.3);
		await page.mouse.down();
		await page.mouse.move(box.x + box.width * 0.6, box.y + box.height * 0.3, { steps: 10 });
		await page.mouse.up();
		await expect.poll(async () => (await keysOf()).some((k) => k.at === inPath), 'the keyframe in the path is drawn over').toBe(false);
		const overdrawn = (await keysOf()).map((k) => k.at);
		expect([overdrawn[0], overdrawn[overdrawn.length - 1]], 'the keyframes outside the path stay').toEqual([outer, other]);
		expect(overdrawn.length).toBeGreaterThan(3);
		// The drag draws over its own keyframes too: it goes out high, comes back low without a
		// lift, and where it came back the high keyframes of the way out are gone. The keyframe
		// it turned at stays.
		await rawCall(page, 'timeline track edit', { track: 'track1', keys: [] });
		await expect(attr.locator('.key')).toHaveCount(0);
		await page.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.2);
		await page.mouse.down();
		await page.mouse.move(box.x + box.width * 0.6, box.y + box.height * 0.2, { steps: 12 });
		await page.mouse.move(box.x + box.width * 0.6, box.y + box.height * 0.8, { steps: 4 });
		await page.mouse.move(box.x + box.width * 0.4, box.y + box.height * 0.8, { steps: 12 });
		await page.mouse.up();
		await expect.poll(async () => (await keysOf()).filter((k) => k.value < 0.5).length, 'the way back made keyframes').toBeGreaterThan(2);
		const [high, low] = [(await keysOf()).filter((k) => k.value > 0.5).map((k) => k.at), (await keysOf()).filter((k) => k.value < 0.5).map((k) => k.at)];
		expect(high.filter((at) => at > Math.min(...low) && at < Math.max(...low)), 'no keyframe of the way out is left where the drag came back').toEqual([]);
		expect(high.filter((at) => at > Math.max(...low)).length, 'the keyframe the drag turned at stays').toBe(1);
		expect(high.some((at) => at < Math.min(...low)), 'the way out stays where the drag did not come back').toBe(true);
		await rawCall(page, 'timeline track edit', { track: 'track1', keys: [first, last] });
		await expect(attr.locator('.key')).toHaveCount(2);
		await dragBy(page, attr.locator('.key').nth(1), box.width * 0.1, box.height * 0.2);
		await expect.poll(async () => (await keysOf())[1].value).toBeLessThan(last.value);
		expect((await keysOf())[1].at).toBeGreaterThan(last.at);
		// A press that hardly moves picks the keyframe and edits nothing. A right click on it opens
		// the menu of what is picked, with its value and its curve; the track's own menu stays shut.
		const held = (await keysOf())[1];
		const dot = (await attr.locator('.key').nth(1).boundingBox())!;
		await page.mouse.move(dot.x + dot.width / 2 + 3, dot.y + dot.height / 2 + 3);
		await page.mouse.down();
		await page.mouse.move(dot.x + dot.width / 2 + 4, dot.y + dot.height / 2 + 3);
		await page.mouse.up();
		await expect(attr.locator('.key.picked')).toHaveCount(1);
		expect((await keysOf())[1]).toEqual(held);
		await expect(curve.getByLabel('Value'), 'the track\'s settings hold nothing of what is picked').toHaveCount(0);
		await attr.locator('.key').nth(1).click({ button: 'right' });
		await expect(pickedMenu.getByLabel('Value')).toHaveValue(String(held.value));
		await expect(page.locator('.context-menu')).toHaveCount(0);
		expect((await keysOf())[1], 'the right press moved nothing').toEqual(held);
		// Its curve opens in a popover of its own. A press beside them closes both and does nothing
		// else: the lane under the press gets no keyframe.
		// A keyframe's curve is named in full, and `smooth` is one of them: a pick of another and
		// back is the keyframe's own curve each time.
		await expect(pickedMenu.locator('.ui-curve-name')).toHaveText('smooth');
		await pickedMenu.locator('.ui-curve-button').click();
		await expect(page.getByTestId('curve-preset')).toBeVisible();
		await expect(page.getByTestId('curve-preset').locator('button')).toHaveText(['smooth', 'step', 'linear', 'easing', 'custom']);
		for (const name of ['linear', 'smooth']) {
			await page.getByTestId('curve-preset').getByText(name, { exact: true }).click();
			await expect.poll(async () => (await keysOf())[1].curve).toBe(name);
		}
		await page.mouse.click(box.x + box.width * 0.45, box.y + box.height * 0.5);
		await expect(page.getByTestId('curve-preset')).toHaveCount(0);
		await expect(pickedMenu).toHaveCount(0);
		expect((await keysOf()).length, 'the press that closed the popover made nothing').toBe(2);
		// A click on a keyframe with Ctrl held picks one more; a click with no key on one of the
		// picked keyframes picks that one alone, and takes nothing out.
		await attr.locator('.key').nth(0).click({ modifiers: ['ControlOrMeta'] });
		await expect(attr.locator('.key.picked')).toHaveCount(2);
		await attr.locator('.key').nth(0).click();
		await expect(attr.locator('.key.picked')).toHaveCount(1);
		await expect(attr.locator('.key').nth(0)).toHaveClass(/picked/);
		expect((await keysOf()).length).toBe(2);
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
		// The position is a label in the corner beside the ruler, which a click makes a field.
		const time = panel.getByTestId('clock-time');
		await expect(time).toHaveText('0.0');
		// A focused field's ring is inside the bar it stands in: the bar keeps the ring's reach.
		const ringed = await panel.getByTestId('clock-bpm').evaluate((el) => {
			(el as HTMLElement).focus();
			const style = getComputedStyle(el);
			const reach = parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset);
			const [field, bar] = [el.getBoundingClientRect(), el.closest('.ui-bar')!.getBoundingClientRect()];
			return reach > 0 && field.top - reach >= bar.top && field.bottom + reach <= bar.bottom;
		});
		expect(ringed, 'the bar holds the focus ring').toBe(true);
		// The line of the position crosses the ruler and every track. Far past what is in view the
		// view stays where it is and shows no line, and the position's field shows the whole number.
		const line = panel.getByTestId('timeline-playhead');
		const [top, bottom] = [(await panel.getByTestId('timeline-ruler').boundingBox())!, (await panel.locator('section.track').last().boundingBox())!];
		const drawn = (await line.boundingBox())!;
		expect(drawn.y <= top.y + 1 && drawn.y + drawn.height >= bottom.y + bottom.height - 1, 'the line crosses the ruler and every track').toBe(true);
		const lastMark = async (): Promise<number> => Number(await panel.getByTestId('timeline-ruler').locator('.tick').last().innerText());
		const marked = await lastMark();
		await rawCall(page, 'clock set', { time: 1234.56 });
		await expect(time).toHaveText('1234.6');
		await expect(line).toHaveCount(0);
		expect(await lastMark(), 'the view stays').toBe(marked);
		expect(await time.evaluate((el: HTMLInputElement) => el.scrollWidth <= el.clientWidth), 'the field shows the whole position').toBe(true);
		await rawCall(page, 'clock set', { time: 0 });
		await expect(time).toHaveText('0.0');
		await time.click();
		await panel.getByTestId('clock-time-input').fill('3');
		await panel.getByTestId('clock-time-input').press('Enter');
		await expect.poll(() => clock(page, 'time')).toBe(3);
		// A focus that types nothing is no edit: the clock stays where another hand put it.
		await time.click();
		const playhead = panel.getByTestId('timeline-playhead');
		const stood = (await playhead.boundingBox())!.x;
		await rawCall(page, 'clock set', { time: 5 });
		await expect.poll(async () => (await playhead.boundingBox())!.x, 'the new position reaches the page under the focus').not.toBe(stood);
		await panel.getByTestId('clock-bpm').click();
		await expect(time).toHaveText('5.0');
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
		await bpm.fill('60');
		await bpm.press('Enter');
		await expect.poll(() => clock(page, 'bpm')).toBe(60);
		// The section is the clock's own, and no variable: `clock set` says what it is.
		const section = async (): Promise<[number, number] | null> => (await rawCall(page, 'clock set', {})).result.section;
		const span = async (): Promise<number> => {
			const now = await section();
			return now ? now[1] - now[0] : 0;
		};

		// A drag along the ruler marks the section the clock loops in: the position restarts at
		// the section's start.
		const mark = async (): Promise<void> => {
			await page.mouse.move(axis.x + axis.width * 0.25, axis.y + 4);
			await page.mouse.down();
			await page.mouse.move(axis.x + axis.width * 0.5, axis.y + 4, { steps: 6 });
			await page.mouse.up();
			await expect.poll(span).toBeGreaterThan(0);
		};
		await mark();
		expect(await span()).toBeCloseTo(timeAt(0.5) - timeAt(0.25), 1);
		const start = (await section())![0];
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
		expect(await lastMark()).toBeCloseTo(start + (await span()), 1);
		await panel.getByTestId('clock-loop').click();
		await expect.poll(span).toBe(0);
		await expect(panel.getByTestId('timeline-loop')).toHaveCount(0);
		await ruler.dblclick({ position: { x: axis.width / 2, y: 4 } });
		await expect.poll(lastMark).toBe(marked);
		// A click on the ruler outside the section clears it.
		await mark();
		await ruler.click({ position: { x: axis.width * 0.75, y: 4 } });
		await expect.poll(span).toBe(0);

		// The timeline's length is the `dur` field, a variable: with one, a double click on the
		// ruler looks at that much.
		const dur = panel.getByTestId('clock-length');
		for (const [beats, last] of [[6, 6], [0, marked]] as const) {
			await dur.fill(String(beats));
			await dur.press('Enter');
			await expect.poll(() => clock(page, 'length')).toBe(beats);
			await ruler.dblclick({ position: { x: axis.width / 2, y: 4 } });
			await expect.poll(lastMark).toBe(last);
			// A line across the tracks marks where the timeline begins, and one where it ends
			// when it has a length: in a view of all of it, at the two ends of the lanes.
			const edges = panel.getByTestId('timeline-edge');
			await expect(edges).toHaveCount(beats > 0 ? 2 : 1);
			const at = await edges.evaluateAll((all) => all.map((el) => Math.round(el.getBoundingClientRect().left)));
			expect(at).toEqual([Math.round(axis.x), ...(beats > 0 ? [Math.round(axis.x + axis.width)] : [])]);
		}

		// The range bar is the view: its end dragged in zooms in, and dragged out past the end of
		// the bar zooms out past everything there is.
		await rawCall(page, 'clock set', { playing: false, time: 0 });
		await expect(time).toHaveText('0.0');
		await expect.poll(lastMark).toBe(marked);
		const bar = (await panel.getByTestId('range-bar').boundingBox())!;
		expect(bar.y + bar.height, 'the range bar is right above the ruler').toBeLessThanOrEqual(axis.y + 1);
		// The bar is as long as a lane, and a view of everything there is fills it from end to end.
		const thumb = panel.getByTestId('range-bar').locator('.thumb');
		const filled = (await thumb.boundingBox())!;
		expect([Math.round(bar.x), Math.round(bar.width)], 'the bar is as long as the ruler').toEqual([Math.round(axis.x), Math.round(axis.width)]);
		expect([Math.round(filled.x), Math.round(filled.width)], 'the view of everything fills the bar').toEqual([Math.round(bar.x), Math.round(bar.width)]);
		await dragBy(page, panel.getByTestId('range-to'), -bar.width / 2, 0);
		await expect.poll(lastMark).toBeLessThan(6);
		await expect(thumb).toHaveAttribute('data-folded', 'false');
		// Its end dragged past the end of the bar folds there: the thumb stays on the bar and its
		// end takes a muted accent, while the view goes on past everything there is.
		await dragBy(page, panel.getByTestId('range-to'), bar.width * 0.6, 0);
		await expect.poll(lastMark).toBeGreaterThan(marked);
		await expect(thumb).toHaveAttribute('data-folded', 'true');
		const folded = (await thumb.boundingBox())!;
		expect(Math.round(folded.x + folded.width), 'the folded thumb ends where the bar ends').toBe(Math.round(bar.x + bar.width));
		expect(await thumb.locator('.fold.after').evaluate((el) => el.getBoundingClientRect().width)).toBeGreaterThan(0);
		await ruler.dblclick({ position: { x: axis.width / 2, y: 4 } });
		await expect.poll(lastMark).toBe(marked);
		await expect(thumb).toHaveAttribute('data-folded', 'false');

		// A wheel turned along the axis over a lane moves the view, as far as the turn is long: the
		// thumb goes that far along the bar and stays as long. A turn across the axis moves nothing.
		await dragBy(page, panel.getByTestId('range-to'), -bar.width / 2, 0);
		await expect.poll(lastMark).toBeLessThan(6);
		const half = (await thumb.boundingBox())!;
		const under = (await curve.getByTestId('track-lane').boundingBox())!;
		await page.mouse.move(under.x + under.width / 2, under.y + under.height / 2);
		await page.mouse.wheel(0, 40);
		expect(Math.round((await thumb.boundingBox())!.x), 'a turn across the axis leaves the view').toBe(Math.round(half.x));
		await page.mouse.wheel(axis.width / 4, 0);
		await expect.poll(async () => Math.round((await thumb.boundingBox())!.x - half.x)).toBe(Math.round(bar.width / 8));
		expect(Math.round((await thumb.boundingBox())!.width), 'the view is as long as it was').toBe(Math.round(half.width));
		// A wheel that turns one way goes along the axis with Shift held: here, back to where it was.
		await page.keyboard.down('Shift');
		await page.mouse.wheel(0, -axis.width / 4);
		await page.keyboard.up('Shift');
		await expect.poll(async () => Math.round((await thumb.boundingBox())!.x)).toBe(Math.round(half.x));
		await panel.locator('.scroll').evaluate((el) => (el.scrollTop = 0));
		// A wheel turned with Ctrl held zooms about the pointer, which is at the middle of the lane:
		// up looks closer, down as far back, and the middle of the view stays where it is.
		const middle = (b: { x: number; width: number }): number => Math.round(b.x + b.width / 2);
		await page.keyboard.down('Control');
		await page.mouse.wheel(0, -200);
		await expect.poll(async () => (await thumb.boundingBox())!.width).toBeLessThan(half.width - 2);
		expect(middle((await thumb.boundingBox())!), 'the point under the pointer stays').toBe(middle(half));
		await page.mouse.wheel(0, 200);
		await page.keyboard.up('Control');
		await expect.poll(async () => Math.round((await thumb.boundingBox())!.width)).toBe(Math.round(half.width));
		// A view moved before the timeline's beginning shows the lane there, and a click there
		// makes a keyframe before 0, as one makes a keyframe past the timeline's length.
		const keysWere = await keysOf();
		await page.mouse.wheel(-axis.width / 4, 0);
		const zero = panel.getByTestId('timeline-edge').first();
		await expect.poll(async () => Math.round((await zero.boundingBox())!.x - axis.x)).toBeGreaterThan(40);
		await page.mouse.click((await zero.boundingBox())!.x - 20, under.y + under.height / 2);
		await expect.poll(async () => (await keysOf()).filter((k) => k.at < 0).length).toBe(1);
		await rawCall(page, 'timeline track edit', { track: 'track1', keys: keysWere });
		// A double click on the bar looks at everything again, as one on the ruler does.
		await panel.getByTestId('range-bar').dblclick();
		await expect.poll(lastMark).toBe(marked);
		await expect.poll(async () => Math.round((await zero.boundingBox())!.x)).toBe(Math.round(axis.x));

		// A track's edge dragged resizes the track in this panel, and changes nothing of the track.
		const tall = async (): Promise<number> => (await curve.locator('.lane').boundingBox())!.height;
		const [was, tracks] = [await tall(), JSON.stringify((await backendDoc(page)).timeline)];
		await dragBy(page, curve.getByTestId('track-grip'), 0, 40);
		await expect.poll(tall).toBeGreaterThan(was + 30);
		// Held at the end of the track area, the edge goes on: the area scrolls under it, and the
		// track is larger than the panel shows, with no other track made smaller for it.
		const area = (await panel.locator('.scroll').boundingBox())!;
		const others = async (): Promise<number> => (await panel.getByTestId('track-lead').locator('.lane').boundingBox())!.height;
		const [grip, beside] = [(await curve.getByTestId('track-grip').boundingBox())!, await others()];
		await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
		await page.mouse.down();
		await page.mouse.move(grip.x + grip.width / 2, area.y + area.height - 4, { steps: 6 });
		await expect.poll(tall).toBeGreaterThan(area.height);
		await page.mouse.up();
		expect(await panel.locator('.scroll').evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
		expect(await others()).toBe(beside);
		await panel.locator('.scroll').evaluate((el) => (el.scrollTop = 0));
		// The edge pressed twice gives the track the room the area shows under the head, less the
		// room the area keeps after its last track, and the area scrolls to put the track's top
		// under the head: its end stands clear of the area's by that room, each to the pixel a
		// scroll lands on. The edge is still the track's to drag.
		await curve.getByTestId('track-grip').dblclick();
		const headRow = (await panel.locator('.row.head').boundingBox())!;
		const spare = await panel.locator('.stack').evaluate((el) => parseFloat(getComputedStyle(el).marginBottom));
		expect(spare).toBeGreaterThan(0);
		const astray = async (): Promise<number[]> => {
			const fitted = (await curve.boundingBox())!;
			return [fitted.y - (headRow.y + headRow.height), fitted.y + fitted.height - (area.y + area.height - spare)];
		};
		await expect.poll(async () => (await astray()).every((d) => Math.abs(d) <= 1), `the track is off the room by ${await astray()}`).toBe(true);
		const fittedTall = await tall();
		await dragBy(page, curve.getByTestId('track-grip'), 0, -40);
		await expect.poll(tall).toBeLessThan(fittedTall - 30);
		await panel.locator('.scroll').evaluate((el) => (el.scrollTop = 0));
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
		await rawCall(page, 'clock set', { section: [0, 0] });
		for (const [name, value] of [['length', 0], ['bpm', 120]] as const) await rawCall(page, 'variable entry edit', { name: `clock.${name}`, value });
		await rawCall(page, 'clock set', { playing: true });
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await closeSplit(page);
		await restorePanelType(page);
	}
});

test('a held finger opens a track\'s menu or a note\'s, and a tap makes a note and then takes it out', async ({ page, hasTouch }) => {
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
		// Inside a cell, clear of the line it begins on and of the end a drag stretches: a tap
		// makes the note, and a tap on the note takes it out.
		const tap = (): Promise<void> => page.touchscreen.tap(lane.x + lane.width * 0.3, lane.y + lane.height * 0.505);
		for (const notes of [1, 0, 1]) {
			await tap();
			await expect.poll(async () => (await trackNamed(page, 'keys')).notes.length).toBe(notes);
		}
		// A finger held on the note opens the note's menu, where a mouse has its right click, and
		// neither moves the note nor takes it out: the menu has the note's settings.
		const finger = await touchSession(page);
		const [on, was] = [(await track.locator('.note').boundingBox())!, (await trackNamed(page, 'keys')).notes];
		await finger.down({ x: on.x + on.width / 2, y: on.y + on.height / 2 });
		await expect(page.getByTestId('picked-menu')).toBeVisible();
		await finger.up();
		await expect(page.locator('.context-menu'), 'the note\'s menu, and not the track\'s').toHaveCount(0);
		expect((await trackNamed(page, 'keys')).notes).toEqual(was);
		await expect(track.locator('.note.picked')).toHaveCount(1);
		await expectIntact(page, 'a note\'s menu on a phone');
		await expect(async () => {
			await page.keyboard.press('Escape');
			expect(await page.getByTestId('picked-menu').count()).toBe(0);
		}).toPass();
		const head = (await track.locator('.info').boundingBox())!;
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
		await expect(dots.nth(0), 'the dot under the finger is the one picked').toHaveClass(/picked/);
		await expect(dots.nth(1)).not.toHaveClass(/picked/);
		// A finger held on a keyframe opens the menu of what is picked, as on a note.
		await finger.down({ x: a.x + a.width / 2, y: a.y + a.height / 2 });
		await expect(page.getByTestId('picked-menu').getByLabel('Value')).toBeVisible();
		await finger.up();
		expect((await trackNamed(page, 'fade')).keys, 'the held finger moved nothing').toEqual(near);
		// A keyframe's menu has the way to take it out: a tap on a keyframe picks it.
		await page.getByTestId('picked-delete').tap();
		await expect.poll(async () => (await trackNamed(page, 'fade')).keys.length).toBe(1);
		await expect(page.getByTestId('picked-menu')).toHaveCount(0);
		await expectIntact(page, 'the timeline panel on a phone');
		// A title too narrow for every action keeps hide and delete in its menu, which closes when
		// one of them is taken.
		await fade.getByTestId('track-more').click();
		const more = page.getByTestId('track-more-menu');
		await more.getByTestId('track-delete').click();
		await expect(more).toHaveCount(0);
		await page.getByTestId('track-delete-dialog').getByRole('button', { name: 'Cancel' }).click();
		await fade.getByTestId('track-more').click();
		await more.getByTestId('track-hide').click();
		await expect(more).toHaveCount(0);
		await expect(fade).toHaveCount(0);
		await expect(page.getByTestId('timeline-hidden')).toBeVisible();
	} finally {
		await rawCall(page, 'clock set', { playing: true });
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await restorePanelType(page);
	}
});

test('a track records its input into itself: a curve an expression, notes a MIDI device', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		await rawCall(page, 'clock set', { playing: false, time: 0 });
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[0].panelId, 'timeline');
		});
		await rawCall(page, 'variable entry add', { name: 'desk.level', value: 0 });
		await rawCall(page, 'timeline track add', { kind: 'curve', name: 'take' });
		const take = page.getByTestId('track-take');
		// The record button is on its section's line, beside the lane or in the track's menu.
		const record = (track: Locator): Promise<Locator> => settings(page, track, '[data-testid="track-record"]');
		// A track with no input says so, and records nothing.
		await (await record(take)).click();
		await expect(page.getByText('has no `input` to record from')).toBeVisible();
		await expect(await record(take)).toHaveAttribute('aria-pressed', 'false');
		// The input is an expression, as a param's; the rate is the track's own.
		await rawCall(page, 'timeline track edit', { track: 'take', input: 'vars.desk.level' });
		await expect(await settings(page, take, '[data-testid="track-input"]')).toContainText('input');
		await shut(page);
		const rate = await settings(page, take, '[data-testid="track-rate"]');
		await rate.fill('10');
		await rate.press('Enter');
		await expect.poll(async () => (await trackNamed(page, 'take')).rate).toBe(10);
		await shut(page);
		// The button begins the recording, lit while it runs, and the paused clock plays for it.
		await (await record(take)).click();
		await expect(await record(take)).toHaveAttribute('aria-pressed', 'true');
		await expect.poll(() => clock(page, 'playing')).toBe(1);
		await rawCall(page, 'variable entry edit', { name: 'desk.level', value: 5 });
		// What the recording takes shows in the lane while it runs: a line through its samples so far.
		await expect(take.getByTestId('track-taking')).toHaveAttribute('d', /^M.+L/);
		// The button again ends it: the keyframes follow what the input held, and the range holds it.
		await (await record(take)).click();
		await expect(await record(take)).toHaveAttribute('aria-pressed', 'false');
		await shut(page);
		await expect.poll(async () => (await trackNamed(page, 'take')).keys.at(-1)?.value).toBe(5);
		await expect(take.getByTestId('track-taking'), 'the take is the track\'s now').toHaveAttribute('d', '');
		const taken = await trackNamed(page, 'take');
		expect([taken.keys[0].value, taken.range], 'from the level it began at').toEqual([0, [0, 5]]);
		await expect(take.locator('.key').first()).toBeVisible();
		// A note track records a MIDI device, and this host has none to offer: its input is the one
		// there will be.
		await rawCall(page, 'timeline track add', { kind: 'note', name: 'played' });
		await expect((await settings(page, page.getByTestId('track-played'), '[data-testid="track-device"] select')).locator('option:checked')).toHaveText('auto');
		await shut(page);
		await (await record(page.getByTestId('track-played'))).click();
		await expect(page.getByText('no MIDI device is connected')).toBeVisible();
		await expectIntact(page, 'the timeline panel with a track that recorded');
	} finally {
		await rawCall(page, 'clock set', { playing: true });
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await rawCall(page, 'variable group remove', { group: 'desk' });
		await restorePanelType(page);
	}
});

test('a node dropped on a curve track\'s input is what the track records', async ({ page, hasTouch }) => {
	test.skip(hasTouch, 'a node onto a row is a mouse drag between two panels; the phone has one');
	await page.goto('/');
	await waitForApp(page);
	const lfo = await addNode(page, 'LFO', [40, 40]);
	await waitForNode(page, lfo);
	try {
		// The editor on the left with a node in it, the timeline on the right with a curve track.
		await splitRight(page);
		await page.evaluate(() => {
			const g = (window as any).goofi;
			g.commands.setPanelType(g.query.panels()[1].panelId, 'timeline');
		});
		await rawCall(page, 'timeline track add', { kind: 'curve', name: 'take' });
		const take = page.getByTestId('track-take');
		// The input is a param's own row, in the recording's section: a node carried over it lights
		// it, and the drop makes the node's output the input, as it does on a param.
		const row = await settings(page, take, '[data-testid="track-input"]');
		const header = page.locator(`.svelte-flow__node[data-id="${lfo}"] .header`);
		const [a, b] = [(await header.boundingBox())!, (await row.boundingBox())!];
		await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
		await page.mouse.down();
		await page.mouse.move(a.x + a.width / 2 + 30, a.y + a.height / 2 + 30, { steps: 4 });
		await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2, { steps: 12 });
		await expect(row).toHaveClass(/over/);
		await page.mouse.up();
		const name = (await backendDoc(page)).nodes[lfo].name;
		await expect.poll(async () => (await trackNamed(page, 'take')).input).toBe(`nd('${name}')`);
	} finally {
		for (const id of Object.keys((await backendDoc(page)).timeline ?? {})) await rawCall(page, 'timeline track remove', { track: id });
		await closeSplit(page);
		await restorePanelType(page);
		await rawCall(page, 'node remove', { node: lfo });
	}
});
