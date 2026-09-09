import { describe, it, expect } from 'vitest';
import { metaEntries, formatMetaValue } from './metaFormat';

describe('metaEntries', () => {
	it('returns top-level entries in insertion order', () => {
		expect(metaEntries({ b: 1, a: 2 })).toEqual([
			['b', 1],
			['a', 2]
		]);
	});
	it('is empty for null/undefined/non-object', () => {
		expect(metaEntries(null)).toEqual([]);
		expect(metaEntries(undefined)).toEqual([]);
	});
	it('hides __*__-namespaced internal-marker keys', () => {
		expect(metaEntries({ sfreq: 250, __internal__: { stats: {} } })).toEqual([['sfreq', 250]]);
	});
});

describe('formatMetaValue', () => {
	it('renders scalars as plain text (no quotes)', () => {
		expect(formatMetaValue(250)).toBe('250');
		expect(formatMetaValue('EEG')).toBe('EEG');
		expect(formatMetaValue(true)).toBe('true');
		expect(formatMetaValue(null)).toBe('null');
	});

	it('renders a list inline on a single line (the DOM word-wraps it to fill width)', () => {
		expect(formatMetaValue([1, 2, 3])).toBe('[1, 2, 3]');
		expect(formatMetaValue(['Fz', 'Cz', 'Pz'])).toBe('[Fz, Cz, Pz]');
		expect(formatMetaValue([])).toBe('[]');
	});

	it('keeps lists inline even nested inside an object', () => {
		expect(formatMetaValue({ ch_names: ['Fz', 'Cz'], sfreq: 250 })).toBe('ch_names: [Fz, Cz]\nsfreq: 250');
	});

	it('renders nested dicts as indented multi-line text', () => {
		expect(formatMetaValue({ info: { highpass: 0.1, lowpass: 40 } })).toBe(
			'info:\n  highpass: 0.1\n  lowpass: 40'
		);
		expect(formatMetaValue({})).toBe('{}');
	});

	it('renders a list of dicts inline (it is still a list)', () => {
		expect(formatMetaValue([{ a: 1 }, { a: 2 }])).toBe('[{a: 1}, {a: 2}]');
	});

	it('formats bigint without throwing (msgpack int64)', () => {
		expect(formatMetaValue(10n)).toBe('10');
		expect(formatMetaValue([1n, 2n])).toBe('[1, 2]');
	});

	it('keeps long lists complete for the expanded view', () => {
		const values = Array.from({ length: 300 }, (_, i) => i);
		expect(formatMetaValue(values)).toBe('[' + values.join(', ') + ']');
	});

	it('keeps full numeric precision', () => {
		expect(formatMetaValue(3.14159)).toBe('3.14159');
		expect(formatMetaValue([1.23456, 2.5])).toBe('[1.23456, 2.5]');
		expect(formatMetaValue({ highpass: 0.00001 })).toBe('highpass: 0.00001');
	});

	it('renders a typed array (msgpack bin → Uint8Array) like a plain list, not an indexed object', () => {
		expect(formatMetaValue(new Uint8Array([1, 2, 3]))).toBe('[1, 2, 3]');
		expect(formatMetaValue({ buf: new Float32Array([1.5, 2.5]) })).toBe('buf: [1.5, 2.5]');
	});
});

import { reconstructMeta } from './metaFormat';

describe('reconstructMeta (Option C reduction-aware inspector)', () => {
	it('restores original shape + drops envelope co-reduced coord + hides reduced', () => {
		const reducedMeta = {
			shape: [2000],
			dtype: '<f4',
			channels: { dim0: new Array(2000).fill(0) }, // co-reduced artifact
			reduced: { '0': { orig_len: 10000, method: 'envelope' } },
			sfreq: 250
		};
		const out = reconstructMeta(reducedMeta);
		expect(out.shape).toEqual([10000]); // true original length
		expect('reduced' in out).toBe(false); // artifact hidden
		expect((out.channels as Record<string, unknown>).dim0).toBeUndefined(); // dropped
		expect(out.sfreq).toBe(250); // untouched
	});

	it('restores carried orig_coord for a subsampled axis', () => {
		const out = reconstructMeta({
			shape: [3, 100],
			channels: { dim0: ['a', 'c', 'f'], dim1: [] },
			reduced: {
				'0': { orig_len: 6, method: 'subsample', orig_coord: ['a', 'b', 'c', 'd', 'e', 'f'] }
			}
		});
		expect(out.shape).toEqual([6, 100]);
		expect((out.channels as Record<string, unknown>).dim0).toEqual(['a', 'b', 'c', 'd', 'e', 'f']);
	});

	it('returns meta unchanged when there is no reduction', () => {
		const m = { shape: [128], sfreq: 250, channels: {} };
		expect(reconstructMeta(m)).toBe(m);
	});
});
