/**
 * Per-viewer ViewSpec — a compatibility predicate plus a reduction request.
 * The wire shape mirrors Rust `goofi_view::ViewSpec`; `ndim` is a conjunction.
 */
import type { ViewerKind } from './kind';
import { VIEWER_KINDS } from '$lib/api/vocab';

export type ReduceMethod = 'envelope' | 'subsample' | 'area';
/** The sample width a viewer can draw; the stream is 8-bit only where every viewer accepts it. */
export type Depth = 'f32' | 'u8';
export type DimCmp = 'lt' | 'le' | 'eq' | 'ge' | 'gt';
export type ViewDtype = 'array' | 'string' | 'table';

export interface AxisReduce {
	dim: number;
	max: number;
	method: ReduceMethod;
}

export interface DimConstraint {
	dim: number;
	cmp: DimCmp;
	n: number;
}

export interface ViewSpec {
	dtype: ViewDtype;
	ndim: [DimCmp, number][];
	dims: DimConstraint[];
	reduce: AxisReduce[];
	depth?: Depth;
}

/** Floor so a 0-px / collapsed layout never asks for a degenerate reduction. */
export const CAP_FLOOR = 64;
const MAX_ROWS = 512;
const MAX_POINTS = 4096;

function px(v: number): number {
	return Math.max(CAP_FLOOR, Math.round(v) || CAP_FLOOR);
}

/** The dimension counts a kind actually DRAWS. It is what it draws that sizes a reduction, so a
 * frame the kind can only describe is declared separately by `describeSpec` — never with these
 * axes, which would ask a producer for a picture nobody is drawing. */
function ndimOf(kind: ViewerKind): [DimCmp, number][] {
	const d = VIEWER_KINDS.find((k) => k.id === kind)?.draws;
	if (!d) return [];
	if (d[0] === d[1]) return [['eq', d[1]]];
	return d[0] === 0 ? [['le', d[1]]] : [['ge', d[0]], ['le', d[1]]];
}

/** What a kind accepts but cannot draw: it shows a summary of such a frame, so it asks for a small
 * area preview — the same shape an image viewer asks for, so the fold stays homogeneous and takes
 * the largest box per dim rather than falling out to the whole frame. `null` where a kind draws
 * everything it accepts. */
function describeSpec(kind: ViewerKind, w: number, h: number): ViewSpec | null {
	const k = VIEWER_KINDS.find((x) => x.id === kind);
	if (!k?.draws || !k.accepts || k.accepts[1] <= k.draws[1]) return null;
	return {
		dtype: 'array',
		ndim: [
			['ge', k.draws[1] + 1],
			['le', k.accepts[1]]
		],
		dims: [],
		reduce: [
			{ dim: 0, max: h, method: 'area' },
			{ dim: 1, max: w, method: 'area' }
		],
		depth: 'u8'
	};
}

/** Everything one viewer of `kind` at `width`x`height` declares: what it draws, and — where it
 * accepts more than it draws — what it can only describe. */
export function viewSpecsForKind(kind: ViewerKind, width: number, height: number): ViewSpec[] {
	const describe = describeSpec(kind, px(width), px(height));
	return describe ? [viewSpecForKind(kind, width, height), describe] : [viewSpecForKind(kind, width, height)];
}

/** The ViewSpec for a viewer `kind` at `width`×`height` device pixels. */
export function viewSpecForKind(kind: ViewerKind, width: number, height: number): ViewSpec {
	const w = px(width);
	const h = px(height);
	const ndim = ndimOf(kind);
	if (kind === 'line') {
		// For 1-D, dim 0 and -1 collide on the bridge; it resolves by richness (envelope wins).
		return {
			dtype: 'array',
			ndim,
			dims: [],
			reduce: [
				{ dim: 0, max: Math.min(h, MAX_ROWS), method: 'subsample' },
				{ dim: -1, max: w, method: 'envelope' }
			]
		};
	}
	if (kind === 'image') {
		// 8-bit: an image is drawn through a 256-level LUT or as colour bytes either way, so the
		// other three quarters of the bandwidth buy nothing.
		return {
			dtype: 'array',
			ndim,
			dims: [],
			reduce: [
				{ dim: 0, max: h, method: 'area' },
				{ dim: 1, max: w, method: 'area' }
			],
			depth: 'u8'
		};
	}
	if (kind === 'trajectory') {
		// (dims × points): the path is the LAST axis, and a phase portrait has no peaks to keep.
		return {
			dtype: 'array',
			ndim,
			dims: [],
			reduce: [{ dim: -1, max: Math.min(w, MAX_POINTS), method: 'subsample' }]
		};
	}
	if (kind === 'topomap') {
		return { dtype: 'array', ndim, dims: [], reduce: [] };
	}
	if (kind === 'string') {
		return { dtype: 'string', ndim, dims: [], reduce: [] };
	}
	return { dtype: 'table', ndim, dims: [], reduce: [] };
}
