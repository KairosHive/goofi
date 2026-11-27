import { test, expect, type Page } from '@playwright/test';
import { waitForApp, resetPatch } from '../lib/app';
import { addNode, waitForNode } from '../lib/goofi';

/* The cable gestures of the node canvas: a node put into a cable, a cable put onto a cable, the
 * swap between two slots of a node, and the Ctrl-drag that unplugs a slot. Each is asked of the
 * patch the manager holds after the release, and the glow is asked while the drag is held. */

type At = { x: number; y: number };
type Side = 'source' | 'target';

const handle = async (page: Page, uid: string, slot: string, side: Side): Promise<At> => {
	const r = (await page.locator(`.svelte-flow__node[data-id="${uid}"] .svelte-flow__handle.${side}[data-handleid="${slot}"]`).boundingBox())!;
	return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
};

const header = async (page: Page, uid: string): Promise<At> => {
	const r = (await page.locator(`.svelte-flow__node[data-id="${uid}"] .header`).boundingBox())!;
	return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
};

/** The middle of the cable between an output and an input: a cable passes through it. */
const cableMid = async (page: Page, out: [string, string], inp: [string, string]): Promise<At> => {
	const p = await handle(page, out[0], out[1], 'source');
	const q = await handle(page, inp[0], inp[1], 'target');
	return { x: (p.x + q.x) / 2, y: (p.y + q.y) / 2 };
};

/** Press at `from`, move to `to`, run `held` with the button down, and release. */
async function drag(page: Page, from: At, to: At, opts: { ctrl?: boolean; held?: () => Promise<void> } = {}): Promise<void> {
	await page.mouse.move(from.x, from.y);
	if (opts.ctrl) await page.keyboard.down('Control');
	await page.mouse.down();
	await page.mouse.move((from.x + to.x) / 2, (from.y + to.y) / 2 + 6, { steps: 5 });
	await page.mouse.move(to.x, to.y, { steps: 5 });
	await opts.held?.();
	await page.mouse.up();
	if (opts.ctrl) await page.keyboard.up('Control');
}

const link = (page: Page, a: string, slotOut: string, b: string, slotIn: string): Promise<void> =>
	page.evaluate((l) => (window as any).goofi.commands.addLink(l), { node_out: a, slot_out: slotOut, node_in: b, slot_in: slotIn });

test('cables are inserted into, replaced, swapped and unplugged by drags', async ({ page }) => {
	await page.goto('/');
	await waitForApp(page);
	try {
		const names: Record<string, string> = {};
		const born = async (name: string, type: string, pos: [number, number]): Promise<string> => {
			const uid = await addNode(page, type, pos);
			await waitForNode(page, uid);
			names[uid] = name;
			return uid;
		};
		/** The patch's links, each as `node.slot>node.slot` by the names given here, sorted. */
		const links = (): Promise<string[]> =>
			page
				.evaluate(() => (window as any).goofi.query.graph().links as { node_out: string; slot_out: string; node_in: string; slot_in: string }[])
				.then((ls) => ls.map((l) => `${names[l.node_out] ?? '?'}.${l.slot_out}>${names[l.node_in] ?? '?'}.${l.slot_in}`).sort());
		const fit = async (): Promise<void> => {
			await page.locator('.svelte-flow__pane').press('f');
			await expect(page.locator('.svelte-flow__node').first()).toBeInViewport();
		};
		const lit = page.locator('.svelte-flow__edge.cable-drop');
		const picker = page.getByTestId('slot-picker');
		const undo = (): Promise<void> => page.evaluate(() => (window as any).goofi.commands.undo());

		const a = await born('a', 'signal:Constant', [0, 0]);
		const m = await born('m', 'signal:Math', [720, 0]);
		const s = await born('s', 'signal:Splice', [360, 360]);
		await link(page, a, 'out', m, 'input');
		await expect.poll(links).toEqual(['a.out>m.input']);
		await fit();

		await test.step('a node held over a cable lights it, and the release puts the node into it', async () => {
			await drag(page, await header(page, s), await cableMid(page, [a, 'out'], [m, 'input']), {
				held: async () => {
					await expect(lit).toHaveCount(1);
					await expect(lit, 'one kind on both sides: no converter').not.toHaveClass(/convert/);
				}
			});
			await expect.poll(links, 'the first free input and the first output').toEqual(['a.out>s.a', 's.out>m.input']);
			await expect(lit).toHaveCount(0);
		});

		await test.step('several inputs open the popover; a pick moves the cable, a press outside keeps it', async () => {
			await expect(picker).toBeVisible();
			await expect(picker.getByTestId('slot-pick-input')).toHaveCount(3);
			await expect(picker.getByTestId('slot-pick-output')).toHaveCount(1);
			await expect(picker.locator('[data-testid="slot-pick-input"][data-slot="a"]')).toHaveAttribute('aria-checked', 'true');
			await picker.locator('[data-testid="slot-pick-input"][data-slot="mask"]').click();
			await expect.poll(links).toEqual(['a.out>s.mask', 's.out>m.input']);
			const pane = (await page.locator('.svelte-flow__pane').boundingBox())!;
			await page.mouse.click(pane.x + pane.width - 20, pane.y + pane.height - 20);
			await expect(picker).toHaveCount(0);
			await expect.poll(links).toEqual(['a.out>s.mask', 's.out>m.input']);
		});

		await test.step('the insert and its pick are one undo step', async () => {
			await undo();
			await expect.poll(links).toEqual(['a.out>m.input']);
			await page.evaluate(() => (window as any).goofi.commands.redo());
			await expect.poll(links).toEqual(['a.out>s.mask', 's.out>m.input']);
		});

		await test.step('a drag between two inputs of a node swaps their cables, an empty slot included', async () => {
			await drag(page, await handle(page, s, 'mask', 'target'), await handle(page, s, 'a', 'target'));
			await expect.poll(links).toEqual(['a.out>s.a', 's.out>m.input']);
		});

		await test.step('a Ctrl-drag unplugs an input: onto another input it moves, onto the canvas it is removed', async () => {
			await drag(page, await handle(page, s, 'a', 'target'), await handle(page, s, 'b', 'target'), {
				ctrl: true,
				held: async () => {
					await expect(page.getByTestId('unplug-preview').locator('path')).toHaveCount(1);
				}
			});
			await expect.poll(links).toEqual(['a.out>s.b', 's.out>m.input']);
			const b = await handle(page, s, 'b', 'target');
			await drag(page, b, { x: b.x - 60, y: b.y + 160 }, { ctrl: true });
			await expect.poll(links).toEqual(['s.out>m.input']);
			await undo();
			await expect.poll(links).toEqual(['a.out>s.b', 's.out>m.input']);
		});

		const c = await born('c', 'signal:Constant', [0, 600]);
		const n = await born('n', 'signal:Null', [720, 480]);
		await link(page, s, 'out', n, 'input');
		await expect.poll(links).toEqual(['a.out>s.b', 's.out>m.input', 's.out>n.input']);
		await fit();

		await test.step('a Ctrl-drag from an output moves every cable it holds', async () => {
			await drag(page, await handle(page, s, 'out', 'source'), await handle(page, c, 'out', 'source'), {
				ctrl: true,
				held: async () => {
					await expect(page.getByTestId('unplug-preview').locator('path')).toHaveCount(2);
				}
			});
			await expect.poll(links).toEqual(['a.out>s.b', 'c.out>m.input', 'c.out>n.input']);
		});

		await test.step('a cable dragged from an output onto a cable replaces it', async () => {
			await drag(page, await handle(page, a, 'out', 'source'), await cableMid(page, [c, 'out'], [n, 'input']), {
				held: async () => {
					await expect(lit).toHaveCount(1);
					await expect(lit).not.toHaveClass(/convert/);
				}
			});
			await expect.poll(links).toEqual(['a.out>n.input', 'a.out>s.b', 'c.out>m.input']);
		});

		await test.step('a cable dragged from an input onto a cable takes that cable\'s output', async () => {
			await drag(page, await handle(page, s, 'a', 'target'), await cableMid(page, [c, 'out'], [m, 'input']));
			await expect.poll(links).toEqual(['a.out>n.input', 'a.out>s.b', 'c.out>s.a']);
		});

		await test.step('the popover lists a held input; a pick of it replaces its cable, and the next pick gives it back', async () => {
			await drag(page, await header(page, s), await cableMid(page, [a, 'out'], [n, 'input']));
			await expect.poll(links, 'the first free input').toEqual(['a.out>s.b', 'a.out>s.mask', 'c.out>s.a', 's.out>n.input']);
			const held = picker.locator('[data-testid="slot-pick-input"][data-slot="a"]');
			await expect(held).toHaveAttribute('data-free', 'false');
			await held.click();
			await expect.poll(links).toEqual(['a.out>s.a', 'a.out>s.b', 's.out>n.input']);
			await picker.locator('[data-testid="slot-pick-input"][data-slot="mask"]').click();
			await expect.poll(links).toEqual(['a.out>s.b', 'a.out>s.mask', 'c.out>s.a', 's.out>n.input']);
			await page.keyboard.press('Escape');
			await expect(picker).toHaveCount(0);
		});

		const g = await born('g', 'graphics:Gradient', [0, 960]);
		await fit();

		await test.step('a cable of another kind lights the cable in the converter\'s colour and places a converter', async () => {
			await drag(page, await handle(page, g, 'out', 'source'), await cableMid(page, [s, 'out'], [n, 'input']), {
				held: async () => {
					await expect(lit).toHaveClass(/convert/);
				}
			});
			await expect
				.poll(() => page.evaluate(() => ((window as any).goofi.query.graph().nodes as { uid: string; type: string }[]).find((x) => x.type === 'signal:GraphicsIn')?.uid ?? null))
				.not.toBeNull();
			const conv = await page.evaluate(() => ((window as any).goofi.query.graph().nodes as { uid: string; type: string }[]).find((x) => x.type === 'signal:GraphicsIn')!.uid);
			names[conv] = 'conv';
			await expect.poll(links).toEqual(['a.out>s.b', 'a.out>s.mask', 'c.out>s.a', 'conv.out>n.input', 'g.out>conv.input']);
		});
	} finally {
		await page.keyboard.up('Control');
		await page.mouse.up();
		await resetPatch(page);
	}
});
