import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { waitForApp } from '../lib/app';
import { rawCall, backendDoc } from '../lib/raw';
import { addNode, selectNode } from '../lib/goofi';

test('save dialogs and axis presets keep the user in control', async ({ page }) => {
	const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'goofi-save-'));
	await page.goto('/');
	await waitForApp(page);
	try {
		const uid = await addNode(page, 'Buffer');
		await selectNode(page, uid);
		const options = page.getByTestId('param-options');
		await expect(options).toBeVisible();
		await options.getByRole('button', { name: '-2', exact: true }).click();
		await expect.poll(async () => (await backendDoc(page)).nodes[uid].params.buffer.axis.value).toBe(-2);
		const number = options.locator('..').getByTestId('param-number');
		await number.fill('5');
		await number.press('Enter');
		await expect.poll(async () => (await backendDoc(page)).nodes[uid].params.buffer.axis.value).toBe(5);

		const numbers = page.getByTestId('param-rows').getByTestId('param-number');
		expect(await numbers.count()).toBeGreaterThan(1);
		await numbers.nth(0).fill('7');
		await numbers.nth(0).press('Tab');
		await expect(numbers.nth(1)).toBeFocused();
		await numbers.nth(1).press('Shift+Tab');
		await expect(numbers.nth(0)).toBeFocused();
		await expect(numbers.nth(0)).toHaveValue('7');

		const target = path.join(dir, 'held.gfi');
		fs.writeFileSync(target, 'keep this file');
		await page.getByTestId('topbar-save-caret').click();
		await page.getByText('Save As…', { exact: true }).click();
		const browser = page.getByTestId('fs-browser');
		const pathInput = browser.getByTestId('fs-path-input');
		await pathInput.fill(dir);
		await pathInput.press('Enter');
		await browser.getByTestId('fs-filename').fill('held');
		await browser.getByTestId('fs-save').click();
		await expect(page.getByTestId('fs-replace-dialog')).toBeVisible();
		expect(fs.readFileSync(target, 'utf8')).toBe('keep this file');
		await page.getByTestId('fs-replace-dialog').getByRole('button', { name: 'Cancel' }).click();
		await expect(browser).toBeVisible();
		await browser.getByTestId('fs-save').click();
		await page.getByTestId('fs-replace').click();
		await expect(browser).toBeHidden();
		await expect.poll(() => fs.readFileSync(target).subarray(0, 2).toString()).toBe('PK');

		// The browser opens where the last patch went, and the order is this viewer's own pick.
		const big = path.join(dir, 'big.txt');
		fs.writeFileSync(big, 'x'.repeat(1_000_000));
		fs.utimesSync(big, new Date(2000, 0, 1), new Date(2000, 0, 1));
		await page.getByTestId('topbar-load').click();
		await expect(pathInput).toHaveValue(fs.realpathSync(dir));
		await expect(browser.getByTestId('fs-recent').first(), 'and it heads the recent folders').toHaveText(path.basename(dir));
		const rows = browser.getByTestId('fs-entry');
		await expect(rows).toHaveText([/big\.txt/, /held\.gfi/]);
		await browser.getByTestId('fs-sort-modified').click();
		await expect(rows, 'newest first').toHaveText([/held\.gfi/, /big\.txt/]);
		await browser.getByTestId('fs-sort-modified').click();
		await expect(rows, 'a second click turns it').toHaveText([/big\.txt/, /held\.gfi/]);
		await browser.getByTestId('fs-sort-size').click();
		await expect(rows.first(), 'largest first').toContainText('1.0 MB');
		await browser.getByRole('button', { name: 'Cancel' }).click();
		await page.getByTestId('topbar-load').click();
		await expect(browser.getByTestId('fs-sort-size')).toHaveAttribute('aria-pressed', 'true');
		await expect(rows.first()).toContainText('big.txt');
		await browser.getByRole('button', { name: 'Cancel' }).click();

		const status = (await rawCall(page, 'session status')).result;
		const source = path.join(status.workspace, 'nodes_signal', 'save_choice.py');
		fs.mkdirSync(path.dirname(source), { recursive: true });
		fs.writeFileSync(source, 'import goofi\nclass SaveChoice(goofi.Node):\n    OUTPUTS = {"out": goofi.DataType.ARRAY}\n');
		expect((await rawCall(page, 'library refresh')).error).toBeUndefined();
		const custom = await addNode(page, 'SaveChoice', [300, 0]);
		await selectNode(page, custom);
		await page.getByTestId('save-to-library').click();
		await expect.poll(async () => (await rawCall(page, 'library get', { type: 'SaveChoice' })).result.provenance).toBe('custom');
		await expect(page.getByTestId('save-to-library')).toBeVisible();
		await page.getByTestId('save-to-library').click();
		await expect(page.getByTestId('library-replace-dialog')).toBeVisible();
		await page.getByTestId('library-name').fill('save_renamed.py');
		await page.getByTestId('library-rename').click();
		await expect(page.getByTestId('library-replace-dialog')).toBeHidden();
		await expect.poll(async () => (await rawCall(page, 'library get', { type: 'SaveRenamed' })).result?.provenance).toBe('custom');
		await page.getByTestId('save-to-library').click();
		await page.getByTestId('library-replace').click();
		await expect(page.getByTestId('library-replace-dialog')).toBeHidden();
	} finally {
		await rawCall(page, 'session new');
		fs.rmSync(dir, { recursive: true, force: true });
	}
});
