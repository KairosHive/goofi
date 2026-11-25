import { expect, type Page } from '@playwright/test';
import { rawCall } from './raw';

/**
 * The agent-harness door for specs: spawn, read the roster, hand it back.
 *
 * Everything here goes through a RAW `/control` socket because the
 * harness these specs use is `_sh` — a config entry `globalSetup` writes into the scoped
 * `GOOFI_HOME`, a plain `sh`, the same `_`-prefixed idiom the node catalog uses. A `_` entry has
 * no door in the UI on purpose: the launcher never advertises it. So nothing here needs a real
 * agent installed.
 */

export const spawnSh = async (page: Page): Promise<string> =>
	(await rawCall(page, 'agent start', { name: '_sh' })).result.instance_id;

/** An instance's state on the manager's roster, or 'gone' once it has been dismissed. */
export async function stateOf(page: Page, id: string): Promise<string> {
	const roster = (await rawCall(page, 'agent list', {})).result;
	return roster.instances.find((i: { id: string }) => i.id === id)?.state ?? 'gone';
}

/** Stop and then DISMISS an instance, leaving nothing on the roster for the next spec. The second
 * stop is what dismisses, and only an ALREADY-EXITED instance is dismissed by one — so it waits
 * for the exit first. */
export async function dismiss(page: Page, id: string): Promise<void> {
	await rawCall(page, 'agent stop', { instance: id });
	await expect.poll(() => stateOf(page, id)).toMatch(/exited|gone/);
	if ((await stateOf(page, id)) === 'exited') await rawCall(page, 'agent stop', { instance: id });
	await expect.poll(() => stateOf(page, id)).toBe('gone');
}
