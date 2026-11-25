import type { Page } from '@playwright/test';

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
