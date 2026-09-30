/**
 * What one viewer kind IS to the client: its ask of a frame, its settings, what it draws and how.
 * The vocabulary — the kind ids, their dtype and the dims they draw — is the manager's, in
 * `$lib/api/vocab`; a module is the behaviour keyed off one of those words.
 */
import type { Surface } from 'plotluck';
import { VIEWER_KINDS, type ViewerKind } from '$lib/api/vocab';
import type { Drawing } from './drawing';

/** The sample width a viewer can draw; the stream is as narrow as the widest viewer's ask. */
export type Depth = 'f32' | 'f16' | 'u8';
export type DimCmp = 'lt' | 'le' | 'eq' | 'ge' | 'gt';
export type ViewDtype = 'array' | 'string' | 'table';

/** One axis's ask: at most `max` entries, subsampled, or the axis whole. A dim a viewer names
 * neither way it has no opinion on, so a kind states every dim it draws. */
export interface AxisReduce {
	dim: number;
	max: number | 'whole';
}

export interface DimConstraint {
	dim: number;
	cmp: DimCmp;
	n: number;
}

/** Per-viewer ViewSpec: a compatibility predicate plus a reduction request. The wire shape
 * mirrors Rust `goofi_view::ViewSpec`; `ndim` is a conjunction. */
export interface ViewSpec {
	dtype: ViewDtype;
	ndim: [DimCmp, number][];
	dims: DimConstraint[];
	reduce: AxisReduce[];
	depth?: Depth;
	/** The asks on dims 0 and 1 are one box: shrunk by one factor, so a picture keeps its aspect. */
	aspect?: boolean;
}

export type SettingValue = boolean | number | string;
export type SettingType = 'toggle' | 'select' | 'number';

export interface SettingDescriptor {
	key: string;
	label: string;
	type: SettingType;
	default: SettingValue;
	options?: string[];
	min?: number;
	max?: number;
	step?: number;
	/** Only show this control while another setting holds one of these values. */
	showWhen?: { key: string; anyOf: SettingValue[] };
}

/** A bag of explicitly-set setting overrides (or resolved values). */
export type SettingsMap = Record<string, SettingValue>;

export interface ViewerModule {
	readonly id: ViewerKind;
	/** Its cog menu, in order. */
	readonly settings: SettingDescriptor[];
	/** What one viewer `w`×`h` device px wide asks of a frame it draws, under `settings`. */
	ask(w: number, h: number, settings: SettingsMap): ViewSpec;
	/** Whether a frame of `shape` draws under `settings`; its dim count is already in range. */
	renders(shape: number[], settings: SettingsMap): boolean;
	/** The drawing the settings choose, by name; the feed remakes the drawing when it changes. */
	variant(settings: SettingsMap): string;
	/** The drawing of an array kind on a surface; a text kind fills its card with a component. */
	readonly drawing: ((surface: Surface, variant: string) => Drawing) | null;
}

/** The dim counts a kind DRAWS, as the conjunction its ask admits. It is what it draws that sizes
 * a reduction; a frame the kind can only describe is asked for separately, by the registry. */
export function drawnNdim(kind: ViewerKind): [DimCmp, number][] {
	const d = VIEWER_KINDS.find((k) => k.id === kind)?.draws;
	if (!d) return [];
	if (d[0] === d[1]) return [['eq', d[1]]];
	return d[0] === 0 ? [['le', d[1]]] : [['ge', d[0]], ['le', d[1]]];
}

/** The 8-bit ask of a picture: both axes fit one box, so it keeps its aspect. */
export function boxAsk(kind: ViewerKind, w: number, h: number, ndim = drawnNdim(kind)): ViewSpec {
	return {
		dtype: 'array',
		ndim,
		dims: [],
		reduce: [
			{ dim: 0, max: h },
			{ dim: 1, max: w }
		],
		depth: 'u8',
		aspect: true
	};
}

/** Every dim up to `ndim` asked whole. */
export function wholeAsk(kind: ViewerKind, ndim: number): ViewSpec {
	return {
		dtype: 'array',
		ndim: drawnNdim(kind),
		dims: [],
		reduce: Array.from({ length: ndim }, (_, dim) => ({ dim, max: 'whole' }))
	};
}

/** The one show-when rule the toggles share: a manual range shows only while `auto` is off. */
export const manual = (key: string) => ({ key, anyOf: [false] });
