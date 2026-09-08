import { describe, it, expect } from 'vitest';
import { UIStore } from './ui.svelte';

// The editor standdown (undo/redo stands down while an in-panel fx editor owns the keyboard) is
// REF-COUNTED, not a shared boolean: each open editor registers a stable id, and the standdown lifts
// only when the LAST one unregisters. This pins the store side of inspector fixes #2B and #3.
describe('UIStore editor standdown (ref-counted modalOpen)', () => {
	it('is closed with no editors registered', () => {
		const ui = new UIStore();
		expect(ui.modalOpen).toBe(false);
	});

	it('opens while any editor is registered', () => {
		const ui = new UIStore();
		ui.openEditor('a');
		expect(ui.modalOpen).toBe(true);
	});

	it('stays open until the LAST editor closes — collapsing one never lifts another (#2B)', () => {
		const ui = new UIStore();
		ui.openEditor('a');
		ui.openEditor('b');
		ui.closeEditor('a');
		expect(ui.modalOpen).toBe(true); // b still holds the standdown
		ui.closeEditor('b');
		expect(ui.modalOpen).toBe(false);
	});

	it('register/unregister is idempotent (double open counts once, extra close underflows nothing)', () => {
		const ui = new UIStore();
		ui.openEditor('a');
		ui.openEditor('a');
		ui.closeEditor('a');
		expect(ui.modalOpen).toBe(false);
		ui.closeEditor('a');
		expect(ui.modalOpen).toBe(false);
	});
});

// A dropped node reaches the OWNER of what it landed on, and the editor that let it go knows only
// where: one registry serves a panel (no tail) and a marked zone (`owner#tail`) alike.
describe('UIStore node drops', () => {
	const AT = { x: 4, y: 8 };

	it('hands a panel drop to what that panel registered', () => {
		const ui = new UIStore();
		const took: string[] = [];
		ui.onNodeDrop('panel1', (uid) => took.push(uid));
		expect(ui.dropNode('panel1', 'n1', AT)).toBe(true);
		expect(took).toEqual(['n1']);
	});

	it('says nobody claimed a target with no registrant, so the caller can fall back', () => {
		const ui = new UIStore();
		expect(ui.dropNode('panel1', 'n1', AT)).toBe(false);
	});

	it('splits a zone at its first #: the owner takes the drop, and reads the rest itself', () => {
		const ui = new UIStore();
		let got: [string, string] | null = null;
		ui.onNodeDrop('form7', (uid, zone) => (got = [uid, zone]));
		expect(ui.dropNode('form7#lfo/frequency', 'n1', AT)).toBe(true);
		expect(got).toEqual(['n1', 'lfo/frequency']);
	});

	it('carries the drop point, which is where a menu the drop opens hangs', () => {
		const ui = new UIStore();
		let where = { x: 0, y: 0 };
		ui.onNodeDrop('form7', (_uid, _zone, at) => (where = at));
		ui.dropNode('form7#lfo/frequency', 'n1', { x: 120, y: 40 });
		expect(where).toEqual({ x: 120, y: 40 });
	});

	it('forgets a registration on undo, and only that one', () => {
		const ui = new UIStore();
		const undo = ui.onNodeDrop('form7', () => {});
		ui.onNodeDrop('panel1', () => {});
		undo();
		expect(ui.dropNode('form7#a/b', 'n1', AT)).toBe(false);
		expect(ui.dropNode('panel1', 'n1', AT)).toBe(true);
	});

	// Two inspectors can show one node, so the zone names the FORM that drew the row. A stale undo
	// (the losing instance unmounting after the winner registered) must not take the live one away.
	it('keeps the newest registration for an owner, and a stale undo leaves it alone', () => {
		const ui = new UIStore();
		const undoFirst = ui.onNodeDrop('form7', () => {});
		let reached = false;
		ui.onNodeDrop('form7', () => (reached = true));
		undoFirst();
		expect(ui.dropNode('form7#a/b', 'n1', AT)).toBe(true);
		expect(reached).toBe(true);
	});
});
