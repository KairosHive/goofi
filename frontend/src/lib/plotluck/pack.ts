/** Where each pane lands on the atlas: shelves of panes, tallest first, on sheets no side of
 * which passes the limit. Pure, so a layout is the same for the same panes. */

export interface Slot {
	x: number;
	y: number;
}

export interface Sheet {
	width: number;
	height: number;
	/** The slot of each pane that landed on this sheet, by its index in the input. */
	slots: Map<number, Slot>;
}

/** Lay `sizes` (device pixels, each side at most `limit`) out on as few sheets as a shelf packing
 * takes. A sheet is about square, so a wide pane beside a tall one wastes little. */
export function pack(sizes: [number, number][], limit: number): Sheet[] {
	const order = sizes
		.map((_, i) => i)
		.filter((i) => sizes[i][0] > 0 && sizes[i][1] > 0)
		.sort((a, b) => sizes[b][1] - sizes[a][1] || sizes[b][0] - sizes[a][0]);
	const area = order.reduce((sum, i) => sum + sizes[i][0] * sizes[i][1], 0);
	const widest = order.reduce((w, i) => Math.max(w, sizes[i][0]), 0);
	const cap = Math.min(limit, Math.max(widest, Math.ceil(Math.sqrt(area))));
	const sheets: Sheet[] = [];
	let sheet: Sheet | null = null;
	let x = 0;
	let y = 0;
	let shelf = 0;
	for (const i of order) {
		const [w, h] = sizes[i];
		if (sheet && x + w > cap) {
			y += shelf;
			x = 0;
			shelf = 0;
		}
		if (!sheet || y + h > limit) {
			sheet = { width: 0, height: 0, slots: new Map() };
			sheets.push(sheet);
			x = 0;
			y = 0;
			shelf = 0;
		}
		sheet.slots.set(i, { x, y });
		x += w;
		shelf = Math.max(shelf, h);
		sheet.width = Math.max(sheet.width, x);
		sheet.height = y + shelf;
	}
	return sheets;
}
