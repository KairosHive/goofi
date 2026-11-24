/** The viewer kinds, one module each, and everything the app reads across them: which kind a
 * slot resolves to, its settings with their defaults, its asks, and whether a frame draws. */
import type { ArrayData } from '$lib/codec/decode';
import type { Surface } from 'plotluck';
import { DEFAULT_KIND, VIEWER_KINDS, type SlotDtype, type ViewerKind } from '$lib/api/vocab';
import type { Drawing } from './drawing';
import { boxAsk, type SettingsMap, type ViewSpec, type ViewerModule } from './module';
import { line } from './kinds/line';
import { image } from './kinds/image';
import { brain } from './kinds/brain';
import { string } from './kinds/string';
import { table } from './kinds/table';

export type { ViewerKind };

/** Typed as a record over the vocabulary, so a kind without a module is a type error. */
export const MODULES: Record<ViewerKind, ViewerModule> = { line, image, brain, string, table };

/** ARRAY viewer kinds the viewer-type dropdown offers, in order. */
export const ARRAY_KINDS: readonly ViewerKind[] = VIEWER_KINDS.filter((k) => k.dtype === 'ARRAY').map((k) => k.id);

/** The kind a dtype PINS: a STRING slot is always drawn by the string viewer, whatever was
 * stored, and a slot with no pin is one whose kind a viewer may choose. */
export function pinnedKind(dtype: string | null): ViewerKind | null {
	return VIEWER_KINDS.find((k) => k.dtype !== 'ARRAY' && k.dtype === dtype)?.id ?? null;
}

/** The viewer kind to use: a dtype-pinned kind wins over the stored one, and a slot with no
 * choice, or with a kind the vocabulary no longer has, opens with what draws its dtype. */
export function resolveKind(dtype: string | null, stored: ViewerKind | undefined): ViewerKind {
	const known = stored && ARRAY_KINDS.includes(stored) ? stored : undefined;
	return pinnedKind(dtype) ?? known ?? DEFAULT_KIND[(dtype ?? 'ARRAY') as SlotDtype] ?? 'line';
}

/** The kinds the panel's plot surface draws, which is every array kind; their card body is a
 * transparent frame over it, and the text kinds fill it themselves. */
export function drawsOnSurface(kind: ViewerKind): boolean {
	return MODULES[kind].drawing !== null;
}

/** The drawing of an array kind, as its settings' `variant` chose it. */
export function makeDrawing(kind: ViewerKind, surface: Surface, variant: string): Drawing {
	const make = MODULES[kind].drawing;
	if (!make) throw new Error(`a ${kind} viewer draws no surface`);
	return make(surface, variant);
}


/** A kind's declared defaults with the explicit overrides applied on top. */
export function resolveSettings(kind: ViewerKind, overrides: SettingsMap | undefined): SettingsMap {
	const out: SettingsMap = {};
	for (const s of MODULES[kind].settings) out[s.key] = s.default;
	return { ...out, ...(overrides ?? {}) };
}

/** Whether an array of the given shape can be drawn by `kind` under `settings`; a non-array
 * frame always can. */
export function isRenderable(kind: ViewerKind, spec: ArrayData | null, settings: SettingsMap): boolean {
	if (!spec) return true;
	const s = spec.shape;
	const draws = VIEWER_KINDS.find((k) => k.id === kind)?.draws;
	if (!draws) return true;
	if (s.length < draws[0] || s.length > draws[1]) return false;
	return MODULES[kind].renders?.(s, settings) ?? true;
}

/** Floor so a 0-px / collapsed layout never asks for a degenerate reduction. */
export const CAP_FLOOR = 64;

function px(v: number): number {
	return Math.max(CAP_FLOOR, Math.round(v) || CAP_FLOOR);
}

/** What a kind accepts but cannot draw, asked as the image viewer's box so the fold stays homogeneous;
 * `null` where a kind draws everything it accepts. */
function describeSpec(kind: ViewerKind, w: number, h: number): ViewSpec | null {
	const k = VIEWER_KINDS.find((x) => x.id === kind);
	if (!k?.draws || !k.accepts || k.accepts[1] <= k.draws[1]) return null;
	return boxAsk(kind, w, h, [
		['ge', k.draws[1] + 1],
		['le', k.accepts[1]]
	]);
}

/** The ViewSpec for a viewer `kind` at `width`×`height` device pixels. */
export function viewSpecForKind(kind: ViewerKind, width: number, height: number, settings: SettingsMap = {}): ViewSpec {
	return MODULES[kind].ask(px(width), px(height), settings);
}

/** Everything one viewer of `kind` at `width`x`height` declares: what it draws, and — where it
 * accepts more than it draws — what it can only describe. */
export function viewSpecsForKind(kind: ViewerKind, width: number, height: number, settings: SettingsMap = {}): ViewSpec[] {
	const describe = describeSpec(kind, px(width), px(height));
	const draws = viewSpecForKind(kind, width, height, settings);
	return describe ? [draws, describe] : [draws];
}
