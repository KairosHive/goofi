import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { resetPatch, waitForApp } from '../lib/app';
import { addNode, selectNode, waitForNode } from '../lib/goofi';
import { rawCall } from '../lib/raw';

test('each node opens on the param group last fronted on it, else on its first', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		const status = (await rawCall(page, 'session status')).result;
		const source = path.join(status.workspace, 'nodes_signal', 'two_groups.py');
		fs.mkdirSync(path.dirname(source), { recursive: true });
		fs.writeFileSync(source, [
			'import goofi',
			'class TwoGroups(goofi.SignalNode):',
			'    PARAMS = {"shape": {"amount": goofi.NumParam(0.5, 0.0, 1.0)}, "timing": {"rate": goofi.NumParam(1.0, 0.0, 10.0)}}',
			''
		].join('\n'));
		expect((await rawCall(page, 'library refresh')).error).toBeUndefined();
		const a = await addNode(page, 'TwoGroups', [0, 0]);
		const b = await addNode(page, 'TwoGroups', [0, 120]);
		await waitForNode(page, a);
		await waitForNode(page, b);
		const front = page.getByTestId('param-tabs').getByRole('tab', { selected: true });

		await selectNode(page, a);
		await expect(front).toHaveText('shape');
		await page.getByTestId('param-tabs').getByRole('tab', { name: 'timing' }).click();
		await expect(front).toHaveText('timing');

		await selectNode(page, b);
		await expect(front, 'a node never fronted opens on its first group').toHaveText('shape');

		await selectNode(page, a);
		await expect(front, 'a node opens on the group last fronted on it').toHaveText('timing');
	} finally {
		await resetPatch(page);
	}
});

test('a plain vector opens as one list or as its entries', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		const status = (await rawCall(page, 'session status')).result;
		const source = path.join(status.workspace, 'nodes_signal', 'shifted.py');
		fs.mkdirSync(path.dirname(source), { recursive: true });
		fs.writeFileSync(source, 'import goofi\nclass Shifted(goofi.SignalNode):\n    OUTPUTS = {"out": goofi.DataType.ARRAY}\n    PARAMS = {"look": {"shift": goofi.NumParam([0.1, 0.2, 0.3, 0.4], 0.0, 1.0)}}\n');
		expect((await rawCall(page, 'library refresh')).error).toBeUndefined();
		const uid = await addNode(page, 'Shifted', [300, 60]);
		await waitForNode(page, uid);
		await selectNode(page, uid);
		const shift = page.locator('[data-param-key="look/shift"]');
		await expect(shift.getByTestId('param-vector').getByRole('textbox')).toHaveCount(4);
		await shift.getByRole('button', { name: 'shift', exact: true }).click();
		await expect(shift.getByTestId('param-list')).toHaveValue('[0.1, 0.2, 0.3, 0.4]');
		await expect(shift.getByTestId('param-mode')).toBeVisible();
		// As entries, the list and the whole foot leave: each row carries its own source and learn.
		await shift.getByTestId('param-view-individual').click();
		await expect(shift.getByTestId('param-elements').locator('.pf-element')).toHaveCount(4);
		await expect(shift.getByTestId('param-list')).toHaveCount(0);
		await expect(shift.getByTestId('param-mode')).toHaveCount(0);
		await expect(shift.locator('.pf-more').getByRole('button', { name: /midi/i })).toHaveCount(4);
		await shift.getByTestId('param-view-list').click();
		await expect(shift.getByTestId('param-list')).toHaveValue('[0.1, 0.2, 0.3, 0.4]');
		await expect(shift.getByTestId('param-mode')).toBeVisible();
		// A source on the whole shows its editor in the list view only; the entries stand alone, held.
		await rawCall(page, 'node param edit', { node: uid, param: 'look/shift', expression: '0.5' });
		await expect(shift.getByTestId('param-expr-input')).toBeVisible();
		await expect(shift.getByTestId('param-list')).toHaveCount(0);
		await shift.getByTestId('param-view-individual').click();
		await expect(shift.getByTestId('param-expr-input')).toHaveCount(0);
		await expect(shift.getByTestId('param-mode')).toHaveCount(0);
		await expect(shift.getByTestId('param-element-number-0')).toBeDisabled();
		await shift.getByTestId('param-view-list').click();
		await shift.getByTestId('param-mode-constant').click();
		await expect(shift.getByTestId('param-list')).toBeVisible();
	} finally {
		await resetPatch(page);
	}
});
