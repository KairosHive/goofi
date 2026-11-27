import { describe, expect, it } from 'vitest';
import type { NodeTypeInfo } from '$lib/api/control';
import {
	converter,
	converts,
	defaultInput,
	defaultOutput,
	insertEdits,
	moveEdits,
	redirectEdits,
	replaceEdits,
	slotChoices,
	swapEdits,
	type Cable,
	type End
} from './rewire';

const type = (name: string, input: string, output: string): NodeTypeInfo =>
	({ type: name, available: true, input_slots: { input }, output_slots: { out: output } }) as unknown as NodeTypeInfo;
const TYPES = [type('graphics:SignalIn', 'ARRAY', 'TEXTURE'), type('signal:GraphicsIn', 'TEXTURE', 'ARRAY')];

const end = (node: string, slot: string, dtype = 'ARRAY'): End => ({ node, slot, dtype });
const cable = (from: End, to: End): Cable => ({ from, to });
const text = (edits: ReturnType<typeof insertEdits>): string[] | null =>
	edits?.map((e) => `${e.op} ${e.link.node_out}.${e.link.slot_out}>${e.link.node_in}.${e.link.slot_in}${e.op === 'add' && e.via ? ` via ${e.via.type}` : ''}`) ?? null;

describe('converter', () => {
	it('names the seam node of the target plane, and nothing for a kind with no plane', () => {
		expect(converter(TYPES, 'ARRAY', 'TEXTURE')).toEqual({ type: 'graphics:SignalIn', input: 'input', output: 'out' });
		expect(converter(TYPES, 'ARRAY', 'AUDIO')).toBeNull(); // not in this library
		expect(converter(TYPES, 'STRING', 'ARRAY')).toBeNull();
	});
});

describe('a node put into a cable', () => {
	const ab = cable(end('a', 'out'), end('b', 'in'));
	it('takes the cable through its slots', () => {
		expect(text(insertEdits(TYPES, ab, end('x', 'in'), end('x', 'out')))).toEqual(['remove a.out>b.in', 'add a.out>x.in', 'add x.out>b.in']);
	});
	it('places a converter on the side whose kind differs', () => {
		const edits = insertEdits(TYPES, ab, end('x', 'in'), end('x', 'out', 'TEXTURE'));
		expect(text(edits)).toEqual(['remove a.out>b.in', 'add a.out>x.in', 'add x.out>b.in via signal:GraphicsIn']);
		expect(converts(edits!)).toBe(true);
	});
	it('is refused when no converter carries the kind', () => {
		expect(insertEdits(TYPES, ab, end('x', 'in', 'STRING'), end('x', 'out'))).toBeNull();
	});
	it('defaults to the first free input and the first output', () => {
		const node = { uid: 'x', input_slots: { p: 'ARRAY', q: 'ARRAY', r: 'ARRAY' }, output_slots: { u: 'STRING', v: 'ARRAY' }, input_multi: [] };
		const held = [cable(end('c', 'out'), end('x', 'p'))];
		const inputs = slotChoices(TYPES, node, 'input', ab.from, held);
		expect(inputs.map((c) => c.free)).toEqual([false, true, true]);
		expect(defaultInput(inputs)?.slot).toBe('q');
		expect(defaultInput(inputs.slice(0, 1))?.slot).toBe('p'); // none free: the first, replaced
		expect(defaultOutput(slotChoices(TYPES, node, 'output', ab.to, held))?.slot).toBe('v'); // STRING cannot reach
	});
});

describe('a cable dragged onto a cable', () => {
	const ab = cable(end('a', 'out'), end('b', 'in'));
	it('keeps the far end of the cable it replaces', () => {
		expect(text(replaceEdits(TYPES, ab, end('c', 'out'), 'source'))).toEqual(['remove a.out>b.in', 'add c.out>b.in']);
		expect(text(replaceEdits(TYPES, ab, end('d', 'in'), 'target'))).toEqual(['remove a.out>b.in', 'add a.out>d.in']);
	});
	it('converts a kind that differs', () => {
		expect(text(replaceEdits(TYPES, ab, end('c', 'out', 'TEXTURE'), 'source'))).toEqual(['remove a.out>b.in', 'add c.out>b.in via signal:GraphicsIn']);
	});
});

describe('swap and unplug', () => {
	const cables = [cable(end('a', 'o'), end('x', 'p')), cable(end('x', 'u'), end('b', 'i')), cable(end('x', 'u'), end('c', 'i'))];
	it('swaps a held input with an empty one', () => {
		expect(text(swapEdits(cables, 'target', end('x', 'p'), end('x', 'q')))).toEqual(['remove a.o>x.p', 'add a.o>x.q']);
	});
	it('swaps every cable of two outputs', () => {
		const both = [...cables, cable(end('x', 'v'), end('d', 'i'))];
		expect(text(swapEdits(both, 'source', end('x', 'u'), end('x', 'v')))).toEqual([
			'remove x.u>b.i',
			'remove x.u>c.i',
			'remove x.v>d.i',
			'add x.v>b.i',
			'add x.v>c.i',
			'add x.u>d.i'
		]);
	});
	it('refuses a swap between kinds, and has nothing to do for two empty slots', () => {
		expect(swapEdits(cables, 'target', end('x', 'p'), end('x', 'q', 'STRING'))).toBeNull();
		expect(swapEdits(cables, 'target', end('x', 'q'), end('x', 'r'))).toBeNull();
	});
	it('moves every cable of an unplugged output to the slot it is put on', () => {
		expect(text(moveEdits(cables.slice(1), 'source', end('y', 'o')))).toEqual(['remove x.u>b.i', 'remove x.u>c.i', 'add y.o>b.i', 'add y.o>c.i']);
	});
	it('puts an unplugged cable in place of the cable it is dropped on', () => {
		expect(text(redirectEdits(TYPES, cables.slice(0, 1), 'target', cables[1]))).toEqual(['remove a.o>x.p', 'remove x.u>b.i', 'add a.o>b.i']);
	});
});
