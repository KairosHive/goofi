import { test, expect } from '@playwright/test';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { E2E_HOME } from '../playwright.config';
import { resetPatch, waitForApp } from '../lib/app';
import { backendDoc, rawCall } from '../lib/raw';

/** What a goofi that died leaves, once a boot has moved it into the fleet home's recovery
 * folder: `patch.yaml` beside `workspace/` in a nonce directory of a session nobody holds. */
function crashed(manifest: string, home: string | null): string {
	const dir = path.join(
		E2E_HOME, '.goofi', 'system', 'recovery',
		crypto.randomBytes(8).toString('hex'), crypto.randomBytes(16).toString('hex')
	);
	fs.mkdirSync(path.join(dir, 'workspace'), { recursive: true });
	fs.writeFileSync(path.join(dir, 'workspace', 'notes.md'), 'kept');
	fs.writeFileSync(path.join(dir, 'patch.yaml'), manifest);
	fs.writeFileSync(path.join(dir, 'autosave.json'), JSON.stringify({ home, at: Date.now() / 1000 - 120 }));
	return dir;
}

test('a crash is offered at the start: one recovery opened as unsaved work, one removed', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	// The manifest of a patch with one node, minted by the manager itself and taken back off it.
	expect((await rawCall(page, 'node add', { type: 'LFO', pos: [0, 0] })).error).toBeUndefined();
	const manifest = (await rawCall(page, 'session manifest')).result.yaml as string;
	await resetPatch(page);
	const lost = crashed(manifest, '/home/someone/patches/lost.gfi');
	const stray = crashed(manifest, null);
	try {
		await page.reload();
		await waitForApp(page);
		const dialog = page.getByTestId('recover-dialog');
		await expect(dialog).toBeVisible();
		const entries = dialog.getByTestId('recover-entry');
		await expect(entries).toHaveCount(2);
		await expect(dialog.getByText('lost.gfi')).toBeVisible();
		await expect(dialog.getByText('Unsaved patch')).toBeVisible();
		await expect(dialog.getByText('2 min ago').first()).toBeVisible();

		// The ×: the one never saved goes, from the list and from disk; the dialog stays.
		await entries.filter({ hasText: 'Unsaved patch' }).getByTestId('recover-discard').click();
		await expect(entries).toHaveCount(1);
		await expect.poll(() => fs.existsSync(stray)).toBe(false);

		// The row: the patch opens with its old home, unsaved — the title dot says so — and the
		// autosave is gone once it is open.
		await entries.getByRole('button', { name: 'lost.gfi' }).click();
		await expect(dialog).toBeHidden();
		await expect(page).toHaveTitle('● lost.gfi');
		await expect.poll(async () => Object.keys((await backendDoc(page)).nodes).length).toBe(1);
		await expect.poll(() => fs.existsSync(lost)).toBe(false);
		const status = (await rawCall(page, 'session status')).result;
		expect(status.save_path).toBe('/home/someone/patches/lost.gfi');
		expect(fs.readFileSync(path.join(status.workspace, 'notes.md'), 'utf8')).toBe('kept');
	} finally {
		for (const dir of [lost, stray]) fs.rmSync(path.dirname(dir), { recursive: true, force: true });
		await resetPatch(page);
	}
});
