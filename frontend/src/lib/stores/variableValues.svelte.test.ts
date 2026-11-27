import { beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync } from 'svelte';
import type { DataFrame } from '$lib/codec/decode';

/** One stream a reader opened on the data plane: where it goes, and whether it was let go. */
interface Stream {
	node: string;
	slot: string;
	token: string;
	feed: (f: DataFrame) => void;
	open: boolean;
}
const streams: Stream[] = [];

vi.mock('$lib/api/frames', () => ({
	bindViewer: (node: string, slot: string, token: string, _specs: null, feed: (f: DataFrame) => void) => {
		const stream = { node, slot, token, feed, open: true };
		streams.push(stream);
		return () => (stream.open = false);
	}
}));

import { variableForm, variableImage, variableValue, watchVariables } from './variableValues.svelte';

const array = (shape: number[], values: number[]): DataFrame => ({
	dtype: 'ARRAY',
	data: { dtype: '<f4', shape, values: Float32Array.from(values) },
	meta: {}
});
const text = (value: string): DataFrame => ({ dtype: 'STRING', data: value, meta: {} });

/** The open streams, by variable name. */
const open = (): string[] => streams.filter((s) => s.open).map((s) => s.slot);
const send = (name: string, frame: DataFrame): void => {
	streams.find((s) => s.open && s.slot === name)!.feed(frame);
};

/** A component that watches `names`, and the function that unmounts it. */
function watcher(names: () => string[]): () => void {
	const unmount = $effect.root(() => watchVariables(names));
	flushSync();
	return unmount;
}

beforeEach(() => {
	streams.length = 0;
});

describe('the value of a watched variable', () => {
	it('is one stream however many watch it, open until the last of them leaves', () => {
		const first = watcher(() => ['gain']);
		const second = watcher(() => ['gain']);
		expect(streams.map(({ node, slot, token }) => ({ node, slot, token }))).toEqual([
			{ node: 'variables', slot: 'gain', token: 'variable:gain' }
		]);

		send('gain', array([1], [0.5]));
		first();
		expect(open(), 'one watcher is left').toEqual(['gain']);
		expect(variableValue('gain')).toBe(0.5);

		second();
		expect(open()).toEqual([]);
		expect(variableValue('gain'), 'a variable nobody watches has no value').toBe(null);
	});

	it('keeps the stream and the value of a name that stays when the list moves', () => {
		let names = $state(['a', 'b']);
		const unmount = watcher(() => names);
		send('b', array([1], [2]));

		names = ['b', 'c'];
		flushSync();

		expect(open().sort()).toEqual(['b', 'c']);
		expect(streams.filter((s) => s.slot === 'b'), '`b` was never opened a second time').toHaveLength(1);
		expect(variableValue('b')).toBe(2);
		unmount();
		expect(open()).toEqual([]);
	});

	it('reads each frame as the literal and the form it holds', () => {
		const unmount = watcher(() => ['v']);
		expect([variableValue('v'), variableForm('v')], 'before the first frame').toEqual([null, 'number']);

		const cases: Array<[DataFrame, unknown, string]> = [
			[array([1], [3]), 3, 'number'],
			[array([], [4]), 4, 'number'],
			[array([3], [1, 2, 3]), [1, 2, 3], 'list'],
			[array([2, 2], [1, 2, 3, 4]), [[1, 2], [3, 4]], 'list'],
			// Three axes with a last axis that is not four wide: no pad.
			[array([1, 2, 3], [1, 2, 3, 4, 5, 6]), [[[1, 2, 3], [4, 5, 6]]], 'list'],
			[text('sine'), 'sine', 'text'],
			[{ dtype: 'TABLE', data: {}, meta: {} }, null, 'text']
		];
		for (const [frame, value, form] of cases) {
			send('v', frame);
			expect([variableValue('v'), variableForm('v')]).toEqual([value, form]);
			expect(variableImage('v')).toBe(null);
		}
		unmount();
	});

	it('hands a pad out as its array, and nothing else as one', () => {
		const unmount = watcher(() => ['pad']);
		const pad = array([1, 2, 4], [0, 0, 0, 1, 1, 1, 1, 1]);
		send('pad', pad);
		expect(variableForm('pad')).toBe('image');
		expect(variableImage('pad')).toBe(pad.data);
		unmount();
	});
});
