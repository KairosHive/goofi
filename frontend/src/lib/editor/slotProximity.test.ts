import { describe, it, expect } from 'vitest';
import { NODE, inputPorts } from './nodeMetrics';
import { inputAnchors, nearSlots, sameKeys, type AnchorNode } from './slotProximity';

/* The arithmetic of the input-name reveal: while a cable is in flight, the inputs the pointer
 * closes on show their names. It is DOM-free because the component cannot mount in vitest. */

const key = (uid: string, slot: string): string => `${uid}|${slot}`;

describe('nearSlots', () => {
	const anchors = [
		{ key: 'a', x: 0, y: 0 },
		{ key: 'b', x: 100, y: 0 }
	];

	it('takes an anchor inside the radius', () => {
		expect([...nearSlots(anchors, { x: 6, y: 8 }, 20)]).toEqual(['a']);
	});

	it('leaves one outside it alone', () => {
		expect([...nearSlots(anchors, { x: 60, y: 0 }, 20)]).toEqual([]);
	});

	/* Inclusive: the radius names how far the reveal reaches, so a point exactly at it is within
	   reach. A strict `<` would make the boundary a coin flip on floating-point noise. */
	it('takes one exactly ON the boundary', () => {
		expect([...nearSlots(anchors, { x: 12, y: 16 }, 20)], 'a 3-4-5 triangle × 4').toEqual(['a']);
		expect([...nearSlots(anchors, { x: 12.001, y: 16 }, 20)], 'and one hair past it').toEqual([]);
	});

	it('takes every anchor in range, and only those', () => {
		const many = [
			{ key: 'a', x: 0, y: 0 },
			{ key: 'b', x: 0, y: 24 },
			{ key: 'c', x: 0, y: 48 },
			{ key: 'd', x: 0, y: 200 }
		];
		expect([...nearSlots(many, { x: 0, y: 24 }, 25)].sort()).toEqual(['a', 'b', 'c']);
	});

	it('answers empty for no anchors, and for a zero radius off the anchor', () => {
		expect(nearSlots([], { x: 0, y: 0 }, 44).size).toBe(0);
		expect(nearSlots(anchors, { x: 1, y: 0 }, 0).size).toBe(0);
		expect([...nearSlots(anchors, { x: 0, y: 0 }, 0)], 'dead on, at zero radius').toEqual(['a']);
	});
});

describe('sameKeys', () => {
	/* The publisher writes a new Set on every pointermove; without this every move would invalidate
	   every node on the canvas for a set that usually did not change. */
	it('is true only for the same membership', () => {
		expect(sameKeys(new Set(['a', 'b']), new Set(['b', 'a']))).toBe(true);
		expect(sameKeys(new Set(), new Set())).toBe(true);
		expect(sameKeys(new Set(['a']), new Set(['a', 'b'])), 'a different size').toBe(false);
		expect(sameKeys(new Set(['a']), new Set(['b'])), 'the same size, other members').toBe(false);
	});
});

describe('inputAnchors', () => {
	const node = (over: Partial<AnchorNode> = {}): AnchorNode => ({
		uid: 'n1',
		x: 10,
		y: 20,
		slots: ['in', 'other'],
		...over
	});

	/* The anchor is the connector's own centre: `.conn.in` hugs the node's LEFT edge (x = the node
	   position) at the `top` nodeMetrics already computes for the cable anchors — so this reveal and
	   the cable it reveals for are measured from the same point. */
	it('puts each input on the node’s left edge, at nodeMetrics’ own pitch', () => {
		const ports = inputPorts(['in', 'other']);
		expect(inputAnchors([node()], key)).toEqual([
			{ key: 'n1|in', x: 10, y: 20 + ports[0].top },
			{ key: 'n1|other', x: 10, y: 20 + ports[1].top }
		]);
	});

	it('skips a node with no inputs, and keys every anchor through the caller', () => {
		expect(inputAnchors([node({ slots: [] })], key)).toEqual([]);
		expect(inputAnchors([node({ slots: ['in'] })], (u, s) => `${u}/${s}`)[0].key).toBe('n1/in');
	});
});
