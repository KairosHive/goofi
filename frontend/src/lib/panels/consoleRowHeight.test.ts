import { describe, it, expect } from 'vitest';
import { LINE_H, PAD, estimateRowHeight } from './consoleRowHeight';

describe('console row height model', () => {
	it('is the text box plus the padding the row really draws', () => {
		expect(estimateRowHeight(1)).toBe(LINE_H + PAD);
		expect(estimateRowHeight(2)).toBe(2 * LINE_H + PAD);
	});

	it('includes every line of a long message', () => {
		expect(estimateRowHeight(12)).toBe(12 * LINE_H + PAD);
	});

	/* C16. Under the coarse floor every control a row hosts — the node chip, the copy button — is
	   --hit tall, so the ROW is, while its text still says one 16px line. The estimator has to carry
	   that floor or `layout.cum` is ~28px short for every row the ResizeObserver has not reached,
	   which on a long buffer is most of them, and the scrollbar lies about the log's length. */
	it('carries the content floor a coarse pointer imposes, without inflating the padding', () => {
		expect(estimateRowHeight(1, 44)).toBe(44 + PAD);
		// The floor is a MINIMUM, not an override: a tall row is still its own height.
		expect(estimateRowHeight(6, 44)).toBe(6 * LINE_H + PAD);
		// And it is absent by default, which is the fine-pointer answer.
		expect(estimateRowHeight(1)).toBeLessThan(estimateRowHeight(1, 44));
	});
});
