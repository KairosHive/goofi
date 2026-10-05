/** The control panel's geometry in GRID units, never pixels: the panel's width sets a unit.
 * The drag law that turns a widget lives with the widget, in `$lib/ui/knob`. */

import { CONTROL_KINDS, type ControlKindId, type ControlKindInfo } from '$lib/api/vocab';
import type { Literal } from '$lib/api/generated';
import type { Form } from '$lib/stores/variableValues.svelte';

export type Kind = ControlKindId;
/** Each widget kind's value type and the box it is born in, by id. */
export const KIND = Object.fromEntries(CONTROL_KINDS.map((k) => [k.id, k])) as Record<Kind, ControlKindInfo>;

/** Whether a widget of kind `k` can draw a value of `form`, as the manager's `Control::fits` says:
 * a vector any list of numbers, a colour a list of four. */
export function kindFits(k: ControlKindInfo, form: Form, value: Literal | null): boolean {
	if (k.draws === 'any') return true;
	if (k.draws === 'vector' || k.draws === 'color') {
		const numbers = form === 'list' && Array.isArray(value) && value.every((v) => typeof v === 'number');
		return numbers && (k.draws === 'vector' || (value as Literal[]).length === 4);
	}
	return k.draws === form;
}

/** The value a widget of kind `k` starts with when a reader switches an attribute to it. */
export function bornValue(k: ControlKindInfo): Literal {
	switch (k.draws) {
		case 'text': return '';
		case 'vector': return [0, 0, 0];
		case 'color': return [0, 0, 0, 1];
		default: return 0;
	}
}

export interface Cell {
	x: number;
	y: number;
	w: number;
	h: number;
}

/** Pixels to one grid unit, per axis: a row is never shorter than a tap target, so it can be
 * taller than a column is wide. */
export interface Units {
	x: number;
	y: number;
}

/** A cell on the grid, at least one unit each way and never off the left or top edge. */
export function snap(cell: Cell): Cell {
	return {
		x: Math.max(0, Math.round(cell.x)),
		y: Math.max(0, Math.round(cell.y)),
		w: Math.max(1, Math.round(cell.w)),
		h: Math.max(1, Math.round(cell.h))
	};
}

/** `cell` pulled back inside `columns`: the width first, then the left edge. */
function inside(cell: Cell, columns: number): Cell {
	const w = Math.min(cell.w, columns);
	return { ...cell, w, x: Math.min(cell.x, columns - w) };
}

export function sameCell(a: Cell, b: Cell): boolean {
	return a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h;
}

/** The cell a `w × h` widget lands on when dropped with its centre `px, py` pixels into the
 * board. It may cover another — a drop is free — but never hangs off the right edge. */
export function cellAt(px: number, py: number, w: number, h: number, units: Units, columns: number): Cell {
	return inside(snap({ x: px / units.x - w / 2, y: py / units.y - h / 2, w, h }), columns);
}

/** The cell a drag from `origin` by `dx, dy` pixels lands on. */
export function movedBy(origin: Cell, dx: number, dy: number, units: Units, columns: number): Cell {
	return inside(snap({ ...origin, x: origin.x + dx / units.x, y: origin.y + dy / units.y }), columns);
}

/** The cell a resize drag lands on. The origin's x and y stay: a resize moves the far edge only. */
export function resizedBy(origin: Cell, dx: number, dy: number, units: Units, columns: number): Cell {
	const cell = snap({ ...origin, w: origin.w + dx / units.x, h: origin.h + dy / units.y });
	return { ...cell, w: Math.min(cell.w, columns - cell.x) };
}
