import { describe, it, expect } from 'vitest';
import { stepWrapped } from './listNav';

// Every keyboard-walked list steps by this rule, so one assertion covers the add-node menu, the
// combobox and the quick pick alike.
describe('stepWrapped', () => {
	it('walks down and up inside the list', () => {
		expect(stepWrapped(1, 1, 4)).toBe(2);
		expect(stepWrapped(2, -1, 4)).toBe(1);
	});
	it('wraps: down from the last is the first, up from the first is the last', () => {
		expect(stepWrapped(3, 1, 4)).toBe(0);
		expect(stepWrapped(0, -1, 4)).toBe(3);
	});
	it('stays at zero for an empty list', () => {
		expect(stepWrapped(0, 1, 0)).toBe(0);
	});
});
