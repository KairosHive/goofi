import { describe, it, expect } from 'vitest';
import { logImage } from './plotFeed';

describe('logImage', () => {
	it('windows log10 of the values over their positive extent; zero sits at the bottom', () => {
		const { values, lo, hi } = logImage(new Float32Array([0, 1, 10, 1000]), null, { auto: true });
		expect([lo, hi]).toEqual([0, 3]);
		expect(Array.from(values)).toEqual([0, 0, 1, 3]);
	});
	it('maps texels back to the node units before the log, and a manual range through it', () => {
		const { values, lo, hi } = logImage(new Uint8Array([0, 255]), [0, 100], { auto: false, vmin: 1, vmax: 1000 });
		expect([lo, hi]).toEqual([0, 3]);
		expect(Array.from(values)).toEqual([0, 2]);
	});
});
