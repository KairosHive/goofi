// The browser device: an AudioOut that names it puts the sound button in the bar, a press there
// plays the engine's mix in this tab, and the button goes with the device.

import { test, expect } from '@playwright/test';
import { waitForApp } from '../lib/app';
import { rawCall } from '../lib/raw';

test('the browser device is heard in a tab once its button is pressed', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	const made: string[] = [];
	const listening = (): Promise<{ on: boolean; frames: number; error: string | null }> => page.evaluate(() => (window as any).goofi.query.listening());
	try {
		// The device is named at the node's birth: an AudioOut on `default` would open a sound card.
		const osc = await rawCall(page, 'node add', { type: 'audio:Osc' });
		const out = await rawCall(page, 'node add', { type: 'audio:AudioOut', param: [{ name: 'audio/device', value: 'browser' }] });
		expect(osc.error ?? out.error).toBeUndefined();
		made.push(osc.result.name, out.result.name);
		expect((await rawCall(page, 'link add', { from: `${osc.result.name}/out`, to: `${out.result.name}/input` })).error).toBeUndefined();

		// The button is there while the patch's clock is the browser device, and off until a press.
		const button = page.getByTestId('topbar-listen');
		await expect(button).toBeVisible();
		await expect(button).toHaveAttribute('aria-pressed', 'false');
		expect((await listening()).on).toBe(false);
		await button.click();
		await expect(button).toHaveAttribute('aria-pressed', 'true');
		// A second of the mix reached the player, decoded.
		await expect.poll(async () => (await listening()).frames).toBeGreaterThan(48000);
		expect((await listening()).error).toBeNull();
		// A second press is silence, and the socket is let go.
		await button.click();
		await expect(button).toHaveAttribute('aria-pressed', 'false');

		// With the device gone the button goes, and a tab that was playing stops.
		await button.click();
		await expect(button).toHaveAttribute('aria-pressed', 'true');
		await rawCall(page, 'node remove', { node: out.result.name });
		made.pop();
		await expect(button).toHaveCount(0);
		await expect.poll(async () => (await listening()).on).toBe(false);
	} finally {
		for (const node of made) await rawCall(page, 'node remove', { node });
	}
});
