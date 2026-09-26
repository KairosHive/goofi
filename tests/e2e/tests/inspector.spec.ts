import { test, expect } from '@playwright/test';
import { waitForApp } from '../lib/app';
import { addNode, selectNode, updateParam } from '../lib/goofi';

test('a param shown by another leaves the inspector when that one moves, and returns', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	const uid = await addNode(page, 'audio:SignalIn');
	await selectNode(page, uid);
	const waveform = ['mix', 'low', 'high', 'smoothing'].map((name) => page.getByTestId(`param-field-${name}`));
	const pitch = page.getByTestId('param-field-pitch');
	await expect(page.getByTestId('param-field-mode')).toBeVisible();
	for (const field of waveform) await expect(field).toBeVisible();
	await expect(pitch).toHaveCount(0);
	await updateParam(page, uid, 'signal', 'mode', 'oscillator');
	for (const field of waveform) await expect(field).toHaveCount(0);
	await expect(pitch).toBeVisible();
	await expect(page.getByTestId('param-field-mode')).toBeVisible();
	await updateParam(page, uid, 'signal', 'mode', 'waveform');
	for (const field of waveform) await expect(field).toBeVisible();
	await expect(pitch).toHaveCount(0);
});
