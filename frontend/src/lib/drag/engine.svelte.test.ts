import { describe, it, expect } from 'vitest';
import { DragEngine, takes, type DragSource, type Drop, type DropTarget } from './engine.svelte';

// The engine pairs what a source OFFERS with what a target TAKES, by tag, under the pointer. The
// pointer is a point the test maps to an element, so no layout is involved.
const node: DragSource = { key: 'lfo0', tags: ['node', 'number'] };
const text: DragSource = { key: 'desk.name', tags: ['text'] };

function tree(): { panel: HTMLElement; row: HTMLElement; stringRow: HTMLElement; home: HTMLElement; own: HTMLElement } {
	const panel = document.createElement('div');
	const row = document.createElement('div');
	const stringRow = document.createElement('div');
	panel.append(row, stringRow);
	const home = document.createElement('div');
	const own = document.createElement('div');
	home.append(own);
	document.body.append(panel, home);
	return { panel, row, stringRow, home, own };
}

/** An engine whose pointer lands on whatever element `at` names: x picks the element. */
function engineOver(elements: Element[]): DragEngine {
	return new DragEngine((x) => elements[x] ?? null);
}

const taker = (el: Element, tags: DropTarget['tags'], drops: Drop[] = [], id?: string): DropTarget => ({
	el,
	id,
	tags,
	onDrop: (d) => drops.push(d)
});

describe('takes', () => {
	it('is the first of the target’s tags the source offers', () => {
		expect(takes({ tags: ['number', 'node'] }, node)).toBe('number');
		expect(takes({ tags: ['node', 'number'] }, node)).toBe('node');
		expect(takes({ tags: ['text'] }, node)).toBeNull();
	});
	it('lets an exception veto what the tags allow', () => {
		expect(takes({ tags: ['number'], accept: (s) => s.key !== 'lfo0' }, node)).toBeNull();
	});
});

describe('DragEngine', () => {
	it('finds the innermost target under the pointer that takes the source', () => {
		const { panel, row, stringRow } = tree();
		const e = engineOver([row, stringRow]);
		const drops: Drop[] = [];
		e.register(taker(panel, ['node'], drops, 'panel'));
		e.register(taker(row, ['number'], drops, 'row'));
		e.register(taker(stringRow, ['text'], drops, 'string'));
		e.begin(node, { x: 0, y: 0 });
		expect(e.over?.id).toBe('row');
		expect(e.takes('row')).toBe('number');
		expect(e.takes('string')).toBeNull();
		// A string row does not take a number; the panel around it takes the node.
		e.move({ x: 1, y: 0 });
		expect(e.over?.id).toBe('panel');
		expect(e.isOver(panel)).toBe(true);
	});

	it('hands the target the source, the tag it took and the point, then is clear', () => {
		const { row } = tree();
		const e = engineOver([row]);
		const drops: Drop[] = [];
		e.register(taker(row, ['number', 'text'], drops));
		e.begin(text, { x: 0, y: 0 });
		const out = e.drop({ x: 0, y: 3 });
		expect(out?.target?.el).toBe(row);
		expect(drops).toEqual([{ source: text, tag: 'text', at: { x: 0, y: 3 } }]);
		expect(e.source).toBeNull();
		expect(e.over).toBeNull();
		expect(e.point).toBeNull();
	});

	it('seeks no target inside the source’s home, and reports the side it ended on', () => {
		const { home, own, row } = tree();
		const e = engineOver([own, row]);
		const drops: Drop[] = [];
		e.register(taker(own, ['number'], drops));
		e.register(taker(row, ['number'], drops));
		e.begin({ ...node, home }, { x: 0, y: 0 });
		expect(e.inHome).toBe(true);
		expect(e.over).toBeNull();
		expect(e.move({ x: 1, y: 0 })?.inHome).toBe(false);
		expect(e.over?.el).toBe(row);
		const out = e.drop({ x: 0, y: 0 });
		expect(out).toMatchObject({ inHome: true, target: null, start: { x: 0, y: 0 }, at: { x: 0, y: 0 } });
		expect(drops).toEqual([]);
	});

	it('is cancelled by Escape, after which a move and a drop say so', () => {
		const { row } = tree();
		const e = engineOver([row]);
		const drops: Drop[] = [];
		e.register(taker(row, ['number'], drops));
		e.begin(node, { x: 0, y: 0 });
		window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
		expect(e.source).toBeNull();
		expect(e.move({ x: 0, y: 0 })).toBeNull();
		expect(e.drop({ x: 0, y: 0 })).toBeNull();
		expect(drops).toEqual([]);
	});

	it('forgets a target on its removal, and only that one', () => {
		const { row, stringRow } = tree();
		const e = engineOver([row, stringRow]);
		const off = e.register(taker(row, ['number'], [], 'row'));
		e.register(taker(stringRow, ['number'], [], 'other'));
		e.begin(node, { x: 0, y: 0 });
		expect(e.over?.id).toBe('row');
		off();
		expect(e.over).toBeNull();
		expect(e.target('row')).toBeNull();
		expect(e.takes('other')).toBe('number');
	});

	it('reaches a target through a shadow root, from the element under the pointer inside it', () => {
		const host = document.createElement('div');
		document.body.append(host);
		const shadow = host.attachShadow({ mode: 'open' });
		const inner = document.createElement('div');
		shadow.append(inner);
		const e = engineOver([inner]);
		e.register(taker(host, ['node'], [], 'plugin'));
		e.begin(node, { x: 0, y: 0 });
		expect(e.over?.id).toBe('plugin');
	});
});
