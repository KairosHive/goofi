import { describe, expect, it } from 'vitest';
import type { Attribute } from '$lib/api/generated';
import { brief, fractionOf, valueAt } from './attributes';

const num = (vmin: number, vmax: number, int = false): Attribute => ({ default: vmin, kind: { type: 'num', vmin, vmax, int, face: 'plain' }, curve: 'linear' });
const sel: Attribute = { default: 'a', kind: { type: 'string', options: ['a', 'b', 'c'] }, curve: 'linear' };
const bool: Attribute = { default: 0, kind: { type: 'bool' }, curve: 'linear' };

describe('a cell as a slider', () => {
	it('paints a number across its range, an integer on the grid', () => {
		expect(valueAt(num(0, 1), 0.25, 0)).toBe(0.25);
		expect(valueAt(num(-1, 1), 1, 0)).toBe(1);
		expect(valueAt(num(0, 10, true), 0.26, 0)).toBe(3);
		expect(fractionOf(num(0, 10), 2.5)).toBe(0.25);
		expect(fractionOf(num(0, 10), 12)).toBe(1);
	});
	it('paints one entry of a vector and reads it back', () => {
		expect(valueAt(num(0, 1), 0.5, [0.1, 0.2, 0.3], 2)).toEqual([0.1, 0.2, 0.5]);
		expect(fractionOf(num(0, 1), [0.1, 0.2, 0.5], 2)).toBe(0.5);
	});
	it('switches a bool at the middle and walks the options from the top', () => {
		expect(valueAt(bool, 0.6, 0)).toBe(1);
		expect(valueAt(bool, 0.4, 1)).toBe(0);
		expect(fractionOf(bool, 1)).toBe(1);
		expect(valueAt(sel, 1, 'a')).toBe('a');
		expect(valueAt(sel, 0.5, 'a')).toBe('b');
		expect(valueAt(sel, 0, 'a')).toBe('c');
		expect(fractionOf(sel, 'c')).toBe(0);
	});
	it('paints nothing on a text, a colour or an envelope', () => {
		expect(valueAt({ default: '', kind: { type: 'string' }, curve: 'linear' }, 0.5, '')).toBeNull();
		expect(valueAt({ default: [0, 0, 0, 1], kind: { type: 'num', vmin: 0, vmax: 1, int: false, face: 'color' }, curve: 'linear' }, 0.5, [0, 0, 0, 1])).toBeNull();
		expect(fractionOf({ default: '', kind: { type: 'string' }, curve: 'linear' }, 'x')).toBeNull();
	});
	it('prints a value short', () => {
		expect(brief(0.123456)).toBe('0.12');
		expect(brief(3)).toBe('3');
		expect(brief([0.5, 1])).toBe('0.50 1');
		expect(brief('hi')).toBe('hi');
	});
});
