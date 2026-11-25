import { describe, expect, it } from 'vitest';
import { formatNodeLoad, nodeStatsRows, sameStats } from './nodeStats';

describe('formatNodeLoad', () => {
	it('returns null when there are no stats yet', () => {
		expect(formatNodeLoad(null)).toBeNull();
		expect(formatNodeLoad(undefined)).toBeNull();
	});

	it('renders just the rate, narrowing the decimal on large values', () => {
		expect(formatNodeLoad({ updates_per_second: 12.4 })).toBe('12.4 upd/s');
		expect(formatNodeLoad({ updates_per_second: 1000 })).toBe('1000 upd/s');
		expect(formatNodeLoad({ updates_per_second: 0 })).toBe('0.0 upd/s');
	});

	it('renders a graphics node as the share of the frame its pass takes', () => {
		// A pass of 1/120 s in a frame of 1/60 s is half the frame.
		expect(formatNodeLoad({ frame_share: 0.5 })).toBe('50% frame');
		expect(formatNodeLoad({ frame_share: 0.0031 })).toBe('0.3% frame');
		expect(formatNodeLoad({ frame_share: 1.25 })).toBe('125% frame');
		expect(formatNodeLoad({ frame_share: 0 })).toBe('0.0% frame');
	});
});

describe('sameStats', () => {
	it('tells a moved reading, and a reading of another kind, from the one held', () => {
		expect(sameStats({ frame_share: 0.5 }, { frame_share: 0.5 })).toBe(true);
		expect(sameStats({ frame_share: 0.5 }, { frame_share: 0.6 })).toBe(false);
		expect(sameStats({ updates_per_second: 0.5 }, { frame_share: 0.5 })).toBe(false);
		expect(sameStats(null, { updates_per_second: 1 })).toBe(false);
	});
});

describe('nodeStatsRows', () => {
	it('returns an empty list when there are no stats', () => {
		expect(nodeStatsRows(null)).toEqual([]);
		expect(nodeStatsRows(undefined)).toEqual([]);
	});

	it('narrows decimals as magnitudes grow', () => {
		expect(nodeStatsRows({ updates_per_second: 1000 })).toEqual([{ label: 'Update rate', value: '1000 upd/s' }]);
		expect(nodeStatsRows({ updates_per_second: 0 })).toEqual([{ label: 'Update rate', value: '0.0 upd/s' }]);
	});
});

describe('a graphics node', () => {
	it('has a frame time row in place of the update rate', () => {
		expect(nodeStatsRows({ frame_share: 0.5 })).toEqual([{ label: 'Frame time', value: '50%' }]);
	});
});

describe('the drop row', () => {
	it('reports the slot drop rate beside the node update rate', () => {
		expect(nodeStatsRows({ updates_per_second: 12.4 }, 3.2)).toEqual([
			{ label: 'Update rate', value: '12.4 upd/s' },
			{ label: 'Dropped', value: '3.2/s' }
		]);
	});

	it('shows a healthy stream as 0.0/s rather than hiding the row', () => {
		// A stream that is running and dropping nothing is worth SAYING — it is the reading a user
		// checks against. Hiding the row at zero makes its absence ambiguous with "no stream".
		expect(nodeStatsRows({ updates_per_second: 30 }, 0)).toEqual([
			{ label: 'Update rate', value: '30.0 upd/s' },
			{ label: 'Dropped', value: '0.0/s' }
		]);
	});

	it('omits the drop row entirely when no stream is bound', () => {
		expect(nodeStatsRows({ updates_per_second: 12.4 }, null)).toEqual([
			{ label: 'Update rate', value: '12.4 upd/s' }
		]);
		expect(nodeStatsRows({ updates_per_second: 12.4 })).toEqual([
			{ label: 'Update rate', value: '12.4 upd/s' }
		]);
	});
});
