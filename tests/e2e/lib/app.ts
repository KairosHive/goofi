import { expect, type Page } from '@playwright/test';

/** The type a fresh panel is born with, mirroring `frontend/src/lib/api/vocab.ts` — this package
 * imports no frontend source, as `lib/inspector.ts` does with the drag anchors. */
const DEFAULT_PANEL_TYPE = 'node-editor';

/** The bare readiness gate: `window.goofi` is published (AppShell mounted) and the node catalog
 * has arrived over the control WS (`query.nodeTypes()` non-empty ⇒ `hello` landed). On its own
 * this is only for a RELOAD mid-spec, where the spec's own state is legitimately still live —
 * everything else enters through `waitForApp`, which adds the hermeticity backstop. */
export async function appReady(page: Page): Promise<void> {
	await page.waitForFunction(() => {
		const g = (window as any).goofi;
		return !!g && (g.query.nodeTypes()?.length ?? 0) > 0;
	});
}

/** Resolve once the app is fully live AND the shared backend holds a pristine patch again — the
 * readiness gate every spec waits on before driving the façade. */
export async function waitForApp(page: Page): Promise<void> {
	await appReady(page);
	await handBack(page);
}

/**
 * The hermeticity backstop, and it lives here because the readiness gate is the one line every
 * product spec runs before it touches anything.
 *
 * One backend serves every spec on this worker (`globalSetup.ts` spawns one per worker slot, and
 * `fullyParallel: false` keeps a worker's files running one after another), and the patch is
 * MANAGER state: a node, a variable group, a named save path or a panel type a spec leaves behind
 * outlives its page and reaches every later spec on the worker through `hello`. The suite was
 * red five times over such leaks, each time on an innocent file.
 *
 * So every spec starts from the empty patch, which is one manager transaction (`session new`),
 * and then reads back that the workspace is the one a fresh boot shows: one tab, one panel of the
 * default type, no nodes, no name. A spec still hands back what it changed in a `finally`; this
 * is what makes an order of specs, or a spec that died mid-walk, not matter.
 */
async function handBack(page: Page): Promise<void> {
	await resetPatch(page);
	await expect(page.getByTestId('workspace-tabs').locator('.ui-tab'), 'one workspace tab').toHaveCount(1);
	await expect(page.locator('.panel'), 'one panel').toHaveCount(1);
	// The replica pulls the new document a beat after the op answers: `docSynced` is the positive
	// signal that it did, and only after it does an empty node list mean an empty graph.
	await page.waitForFunction(() => (window as any).goofi.query.docSynced());
	await expect
		.poll(() => page.evaluate(() => (window as any).goofi.query.panels()[0]?.type), { message: 'the panel is the default type' })
		.toBe(DEFAULT_PANEL_TYPE);
}

/**
 * Hand the shared backend back an empty, unnamed, clean patch — one manager transaction (`new`).
 *
 * The `finally` half of the save-path contract `waitForApp` hands back at the other end,
 * and the successor to the `loadText(<the file we just saved>)` trick five specs used to play: a
 * `.gfi` is a zip archive now, so the façade carries no content-load door left to abuse for a reset.
 */
export async function resetPatch(page: Page): Promise<void> {
	await page.evaluate(() => (window as any).goofi.commands.newPatch());
	await expect
		.poll(() => page.evaluate(() => (window as any).goofi.query.graph().savePath))
		.toBe(null);
	await expect
		.poll(() => page.evaluate(() => (window as any).goofi.query.graph().nodes.length))
		.toBe(0);
}

/** Hand back a tab the spec added: close the last one. The close IS the command — there is no
 * debounced push to outwait any more — so the arrangement the manager keeps is pristine as soon as
 * the tab strip says so. The `finally` half of the contract `waitForApp` hands back at
 * the other end. */
export async function closeAddedTab(page: Page): Promise<void> {
	const tabs = page.getByTestId('workspace-tabs');
	await tabs.getByRole('button', { name: 'Close tab' }).last().click();
	await expect(tabs.locator('.ui-tab'), 'the workspace is back to one tab').toHaveCount(1);
}

/** Split the sole default panel to the right, through the real header context menu. Two specs need
 *  a two-panel workspace — `panel-surface` to read the seam it paints, `inspector-orientation` to
 *  make the editor a narrow tall column — and a second copy is a second thing to keep true. */
export async function splitRight(page: Page): Promise<void> {
	const header = page.getByTestId('panel-header').first();
	await header.click({ button: 'right' });
	const item = page.locator('.context-menu .item', { hasText: 'Split Right' }).first();
	await expect(item).toBeVisible();
	await item.click();
	await expect(page.getByTestId('panel-header')).toHaveCount(2);
}

/**
 * Put the workspace back, through each extra panel's own ✕. A split is a command against the RUNNING
 * PATCH — one backend per worker — so a spec that splits and leaves early persists a 2-panel
 * workspace that every later spec there boots into. It passes alone and depends on nothing but
 * screenshot latency, which is why it must be a `finally`. It closes whatever a failed body or a
 * retry stacked up, so a `finally` may call it without knowing how many panels are open.
 */
export async function closeSplit(page: Page): Promise<void> {
	await releasePointer(page);
	// Counted by header: `.panel` also matches the inspector's pane when it is open.
	const headers = page.getByTestId('panel-header');
	let open = await headers.count();
	while (open > 1) {
		await headers.nth(1).getByRole('button', { name: 'Close panel' }).click();
		// A closed panel leaves the DOM a beat later; a second click before that lands on it.
		await expect(headers).toHaveCount(open - 1);
		open -= 1;
	}
	await expect(headers, 'the workspace is back to one panel').toHaveCount(1);
}

/** Let go of a drag a failed step left mid-air: a held button swallows the clicks a cleanup makes. */
export async function releasePointer(page: Page): Promise<void> {
	await page.keyboard.press('Escape');
	await page.mouse.up();
}

/**
 * Put the sole panel back on the node editor. A panel's type is a field in the RUNNING PATCH like
 * a split or a tab, so a spec that switches it and leaves early hands every later spec on that
 * worker a page with no canvas on it — and the red lands on whichever project measures a viewer
 * next. The `finally` half of the contract `waitForApp` hands back at the other end.
 */
export async function restorePanelType(page: Page): Promise<void> {
	await page.evaluate((type) => {
		const g = (window as any).goofi;
		g.commands.setPanelType(g.query.panels()[0].panelId, type);
	}, DEFAULT_PANEL_TYPE);
	await expect
		.poll(() => page.evaluate(() => (window as any).goofi.query.panels()[0]?.type))
		.toBe(DEFAULT_PANEL_TYPE);
}
