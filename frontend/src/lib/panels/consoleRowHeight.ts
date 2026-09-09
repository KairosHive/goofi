/** These mirror `.row`'s CSS and stay px: the estimate is computed before layout exists. */
export const LINE_H = 16;
export const PAD = 4;

/** A row's height before measurement, including the content floor for touch controls. */
export function estimateRowHeight(lines: number, floor = 0): number {
	return Math.max(lines * LINE_H, floor) + PAD;
}
