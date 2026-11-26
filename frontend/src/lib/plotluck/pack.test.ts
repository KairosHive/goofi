import { describe, expect, it } from 'vitest';
import { pack } from './pack';

describe('pack', () => {
	it('shelves panes tallest first on a sheet about as wide as it is high', () => {
		const [sheet, ...rest] = pack(
			[
				[400, 300],
				[1600, 900],
				[400, 300],
				[400, 300]
			],
			16384
		);
		expect(rest).toEqual([]);
		expect(sheet.slots.get(1)).toEqual({ x: 0, y: 0 });
		// The three small panes share the shelf under the big one: the cap is the big one's width.
		expect(sheet.slots.get(0)).toEqual({ x: 0, y: 900 });
		expect(sheet.slots.get(2)).toEqual({ x: 400, y: 900 });
		expect(sheet.slots.get(3)).toEqual({ x: 800, y: 900 });
		expect([sheet.width, sheet.height]).toEqual([1600, 1200]);
	});

	it('keeps a layout when a pane is the same, and leaves an empty pane off the sheet', () => {
		const a = pack(
			[
				[300, 200],
				[0, 200],
				[300, 200]
			],
			4096
		);
		const b = pack(
			[
				[300, 200],
				[300, 0],
				[300, 200]
			],
			4096
		);
		expect(a[0].slots.has(1)).toBe(false);
		expect([...a[0].slots]).toEqual([...b[0].slots]);
	});

	it('opens a new sheet where the limit stops a shelf', () => {
		const sheets = pack(
			[
				[1000, 600],
				[1000, 600],
				[1000, 600]
			],
			1000
		);
		expect(sheets.map((s) => [s.width, s.height, s.slots.size])).toEqual([
			[1000, 600, 1],
			[1000, 600, 1],
			[1000, 600, 1]
		]);
	});

	it('has no overlap among many panes of mixed size', () => {
		const sizes: [number, number][] = [];
		for (let i = 0; i < 40; i++) sizes.push([50 + ((i * 97) % 700), 40 + ((i * 61) % 500)]);
		for (const sheet of pack(sizes, 4096)) {
			const boxes = [...sheet.slots].map(([i, s]) => ({ ...s, w: sizes[i][0], h: sizes[i][1] }));
			for (const a of boxes) {
				expect(a.x + a.w).toBeLessThanOrEqual(sheet.width);
				expect(a.y + a.h).toBeLessThanOrEqual(sheet.height);
				for (const b of boxes) {
					if (a === b) continue;
					const apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
					expect(apart).toBe(true);
				}
			}
		}
	});
});
