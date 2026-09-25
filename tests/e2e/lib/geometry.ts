import { expect, type Locator, type Page } from '@playwright/test';

/**
 * Box arithmetic shared by the specs that measure the shell's layout.
 *
 * Both helpers were `touch-reflow.spec.ts`'s privately; the orientation-aware inspector needs the
 * same two questions asked in the OTHER axis, and a spec cannot import another spec (loading a spec
 * file is how Playwright registers its tests). One copy, so a settling read cannot mean two things.
 */

export interface Box {
	x: number;
	y: number;
	width: number;
	height: number;
}

/** Resolve once every finite animation on the page has ended; an infinite one is not waited for. */
export async function animationsDone(page: Page): Promise<void> {
	await page.evaluate(() =>
		Promise.all(
			document
				.getAnimations()
				.filter((a) => a.effect?.getTiming().iterations !== Infinity)
				.map((a) => a.finished.catch(() => undefined))
		)
	);
}

/**
 * A locator's box once its animations have ended and two consecutive reads agree.
 *
 * Anything that slides (the inspector) reports a frame of its animation otherwise, and a position
 * read off a frame is not a measurement of the layout. BOTH axes are compared: the pane slides in X
 * when it is anchored right and in Y when it is a bottom sheet, so an x-only settle is blind to
 * exactly half the cases.
 */
export async function settledBox(loc: Locator): Promise<Box> {
	let prev: Box = { x: NaN, y: NaN, width: 0, height: 0 };
	await animationsDone(loc.page());
	await expect
		.poll(
			async () => {
				const b = (await loc.boundingBox())!;
				const same =
					Math.abs(b.x - prev.x) < 0.5 &&
					Math.abs(b.y - prev.y) < 0.5 &&
					Math.abs(b.width - prev.width) < 0.5 &&
					Math.abs(b.height - prev.height) < 0.5;
				prev = b;
				return same;
			},
			{ message: 'the pane settled' }
		)
		.toBe(true);
	return prev;
}
