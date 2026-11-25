import { expect, type Page } from '@playwright/test';

/**
 * Real touch input, driven through CDP.
 *
 * Not `page.mouse`: under `hasTouch` Playwright's mouse API still dispatches MOUSE events, whose
 * `pointerType` is `mouse` — exactly the input the coarse doors are closed to, so a mouse-driven
 * "long press" would prove nothing. Playwright's own `tap()` is a tap and cannot hold, which is
 * what a long press is.
 *
 * Shared through `lib/` rather than by importing one spec from another, since loading a spec file
 * is how Playwright registers its tests.
 */
export interface TouchPoint {
	x: number;
	y: number;
}

export interface TouchSession {
	down(p: TouchPoint): Promise<unknown>;
	moveTo(p: TouchPoint): Promise<unknown>;
	up(): Promise<unknown>;
}

export async function touchSession(page: Page): Promise<TouchSession> {
	const cdp = await page.context().newCDPSession(page);
	const send = (type: string, touchPoints: TouchPoint[]) =>
		cdp.send('Input.dispatchTouchEvent', { type, touchPoints } as never);
	return {
		down: (p: TouchPoint) => send('touchStart', [p]),
		moveTo: (p: TouchPoint) => send('touchMove', [p]),
		up: () => send('touchEnd', [])
	};
}

/**
 * A finger drag from `a` to `b` that comes to REST before it lifts. Moves dispatched back to back
 * carry a velocity Chromium reads as a fling at release, and it eats the next tap to stop it.
 */
export async function swipe(page: Page, a: TouchPoint, b: TouchPoint, steps = 8): Promise<void> {
	const touch = await touchSession(page);
	await touch.down(a);
	for (let i = 1; i <= steps; i++) {
		await touch.moveTo({
			x: Math.round(a.x + ((b.x - a.x) * i) / steps),
			y: Math.round(a.y + ((b.y - a.y) * i) / steps)
		});
	}
	// The still finger is the gesture; a slow runner only holds it longer.
	await page.waitForTimeout(150);
	await touch.up();
}

/**
 * Two fingers at once, from `gap` apart to `gapTo` apart about `centre`, on a vertical line. Each
 * point carries its id so Chromium keeps the two apart; a d3-zoom pane reads that as a pinch.
 */
export async function pinch(
	page: Page,
	centre: TouchPoint,
	gap: number,
	gapTo: number,
	steps = 8
): Promise<void> {
	const cdp = await page.context().newCDPSession(page);
	const send = (type: string, touchPoints: Array<TouchPoint & { id: number }>) =>
		cdp.send('Input.dispatchTouchEvent', { type, touchPoints } as never);
	const pair = (d: number) => [
		{ id: 0, x: centre.x, y: Math.round(centre.y - d / 2) },
		{ id: 1, x: centre.x, y: Math.round(centre.y + d / 2) }
	];
	await send('touchStart', pair(gap));
	for (let i = 1; i <= steps; i++) await send('touchMove', pair(gap + ((gapTo - gap) * i) / steps));
	// The still fingers are the gesture's end; a fast lift reads as a fling.
	await page.waitForTimeout(150);
	await send('touchEnd', []);
	await cdp.detach();
}

/**
 * A point where the flow pane really is the topmost element AND no node card is within a tap target
 * of it. One backend serves this whole worker, so the patch may carry nodes left by an earlier
 * spec — a hardcoded centre point could land on a node card (which must NOT arm the long-press
 * door) and green a test for the wrong reason.
 *
 * `elementFromPoint` answers "is this bare canvas" exactly. A TOUCH does not ask it that way:
 * Chromium applies **touch adjustment**, snapping a tap within roughly a finger's radius onto a
 * nearby clickable box. Measured here at 10px — a tap 10px clear of a node card's right edge came
 * back as a `click` on the card's edge, so a tap meant to deselect re-selected the node it was
 * beside and the canvas looked dead when it was not. So the point must clear every rendered card by
 * a full `--hit` as well as being provably bare.
 */
export async function emptySpot(page: Page): Promise<TouchPoint> {
	const spot = await bareSpot(page, [0.25, 0.35, 0.45, 0.55, 0.65, 0.75]);
	expect(spot, 'the canvas has some empty space to press on').not.toBeNull();
	return spot!;
}

/** Scan `fys` (fractions of the pane's height, in order) for the first bare, card-clear point. */
async function bareSpot(page: Page, fys: number[]): Promise<TouchPoint | null> {
	return page.locator('.svelte-flow__pane').first().evaluate(
		(pane, fys) => {
			const hit = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--hit'));
			const cards = [...document.querySelectorAll('.svelte-flow__node')].map((n) =>
				n.getBoundingClientRect()
			);
			const clear = (x: number, y: number): boolean =>
				cards.every(
					(c) => x < c.left - hit || x > c.right + hit || y < c.top - hit || y > c.bottom + hit
				);
			const r = pane.getBoundingClientRect();
			for (const fy of fys) {
				for (let fx = 0.2; fx <= 0.8; fx += 0.1) {
					const x = Math.round(r.left + r.width * fx);
					const y = Math.round(r.top + r.height * fy);
					if (document.elementFromPoint(x, y) === pane && clear(x, y)) return { x, y };
				}
			}
			return null;
		},
		fys
	);
}
