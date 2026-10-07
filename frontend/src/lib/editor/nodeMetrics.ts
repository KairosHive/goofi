/** Node geometry, mirroring the `--node-*` custom properties in `app.css` so the connector
 * overlay can place ports without measuring the DOM. Keep the two in step. */
export const NODE = {
	width: 233,
	header: 36,
	unit: 24, // collapsed slot height === input connector pitch
	viewer: 144, // open viewer plot height
	border: 1
} as const;

/** The grid both canvases draw and snap to: one slot unit. `goofi_graph::canvas::GRID` is the same. */
export const GRID = NODE.unit;

/** An output slot's height in px, with its inline viewer open or closed. */
const slotHeight = (open: boolean): number => (open ? NODE.unit + NODE.viewer : NODE.unit);

/** Total input-block height in units, floored at 1 so a node with no inputs still has a body. */
export function inputUnits(slots: string[]): number {
	return Math.max(slots.length, 1);
}

/** The centre (px) of each output connector, below the header, with each slot open or closed. */
export function outputTops(slots: string[], open: (slot: string) => boolean): { slot: string; top: number }[] {
	let y = NODE.border + NODE.header;
	return slots.map((slot) => {
		const top = y + NODE.unit / 2;
		y += slotHeight(open(slot));
		return { slot, top };
	});
}

/** Vertical placement of each input connector: `top` is the centre (px) of its one-unit slot. */
export function inputPorts(slots: string[]): { slot: string; top: number }[] {
	return slots.map((slot, i) => ({ slot, top: NODE.border + NODE.header + (i + 0.5) * NODE.unit }));
}

/** The rendered size of a node's surface box, the snap geometry's fallback until Svelte Flow
 * has measured the node. `outputExpanded[i]` is whether output slot i's inline viewer is open. */
export function nodeSurfaceSize(
	inputUnitsTotal: number,
	outputExpanded: boolean[]
): { width: number; height: number } {
	const slotsStack = outputExpanded.reduce((h, open) => h + slotHeight(open), 0);
	const inputBody = Math.max(inputUnitsTotal, 1) * NODE.unit;
	return { width: NODE.width, height: NODE.header + Math.max(slotsStack, inputBody) };
}
