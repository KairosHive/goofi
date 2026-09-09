import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { waitForApp } from '../lib/app';
import { rawCall } from '../lib/raw';
import { addNode, selectNode, updateParam } from '../lib/goofi';

test('metadata fits inline and expands only when it has more to show', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		const status = (await rawCall(page, 'session status')).result;
		const source = path.join(status.workspace, 'nodes_signal', 'metadata_rows.py');
		fs.mkdirSync(path.dirname(source), { recursive: true });
		fs.writeFileSync(source, `import goofi
import numpy as np
class MetadataRows(goofi.Node):
    PRODUCER = True
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"test": {"shorten": goofi.BoolParam(False)}}
    def process(self):
        return np.array([1.0]), {
            "short": 250,
            "list": [1, 2],
            "object": {"a": 1, "b": 2},
            "long": "done" if self.params.test.shorten else "start " + "long value " * 40,
            "lines": "first line\\nsecond line",
            "coordinates": list(range(300)),
        }
`);
		expect((await rawCall(page, 'library refresh')).error).toBeUndefined();
		const uid = await addNode(page, 'MetadataRows');
		await selectNode(page, uid);
		const row = (key: string) => page.locator(`[data-meta-key="${key}"]`);
		for (const [key, value] of [['short', '250'], ['list', '[1, 2]'], ['object', '{a: 1, b: 2}']]) {
			await expect(row(key).locator('.preview')).toHaveText(value);
			await expect(row(key).getByRole('button')).toHaveCount(0);
		}
		const long = row('long');
		await expect(long.getByRole('button')).toHaveAttribute('aria-expanded', 'false');
		expect(await long.locator('.preview').evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
		await long.getByRole('button').click();
		await expect(long.locator('.preview')).toBeHidden();
		await expect(long.locator('.value')).toHaveText('start ' + 'long value '.repeat(40));
		const headerBox = await long.locator('.row').boundingBox();
		const bodyBox = await long.locator('.value').boundingBox();
		expect(bodyBox!.y).toBeGreaterThanOrEqual(headerBox!.y + headerBox!.height);
		await expect(row('lines').locator('.preview')).toHaveText('first line…');
		await row('lines').getByRole('button').click();
		await expect(row('lines').locator('.value')).toHaveText('first line\nsecond line');
		await row('coordinates').getByRole('button').click();
		await expect(row('coordinates').locator('.value')).toHaveText('[' + Array.from({ length: 300 }, (_, i) => i).join(', ') + ']');
		// Live frames preserve expansion, but a value that fits no longer needs a control.
		await expect(long.getByRole('button')).toHaveAttribute('aria-expanded', 'true');
		await updateParam(page, uid, 'test', 'shorten', true);
		await expect(long.locator('.preview')).toHaveText('done');
		await expect(long.locator('.preview')).toBeVisible();
		await expect(long.getByRole('button')).toHaveCount(0);
		await expect(long.locator('.value')).toHaveCount(0);
	} finally {
		await rawCall(page, 'session new');
	}
});
