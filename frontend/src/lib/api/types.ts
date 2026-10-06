/** Wire shapes for parameter descriptors, assembled from the generated Rust types. */
import type { Mode, ParamBase, ParamEntry, ParamKind, VideoQuality } from './generated';

export type { ParamBase, ParamShow, VideoQuality } from './generated';

/** The one active source of a param's value. */
export type ParamMode = Mode;

/** Every member once: the record is checked against the generated union in both directions. */
const keys = <K extends string>(all: Record<K, 0>): readonly K[] => Object.keys(all) as K[];
export const PARAM_MODES = keys<ParamMode>({ constant: 0, expression: 0 });
export const VIDEO_QUALITIES = keys<VideoQuality>({ small: 0, high: 0, very_high: 0 });

/** What `node param edit` takes beside a value: any subset, and a text given implies its mode. */
export type SourcePatch = Omit<ParamEntry, 'value'>;

type Kind<T extends ParamKind['type']> = ParamBase & Extract<ParamKind, { type: T }>;
/** A number, or a vector of them: `value` is bare for one dimension and a list for more. */
export type NumParam = Kind<'num'>;
export type BoolParam = Kind<'bool'>;
export type StringParam = Kind<'string'>;
export type PulseParam = Kind<'pulse'>;

/** A descriptor for a param whose node type is absent from the catalog: no bounds, no options. */
export type UnknownParam = ParamBase & { type: 'unknown'; value: unknown };

/** One element of a vector param as its own source: `name[i]` in the document, driving that
 * dimension alone. `value` is what it evaluates to while driven. */
export interface ElementSource {
	mode: ParamMode;
	expression: string | null;
	error: string | null;
	value: number | undefined;
}

export type ParamDescriptor = ((ParamBase & ParamKind) | UnknownParam) & {
	/** A vector's elements, each with a source of its own; absent on a scalar. */
	elements?: ElementSource[];
};

/** A number's dimensions, bare or listed, as a list. */
export function numValues(d: NumParam): number[] {
	return Array.isArray(d.value) ? d.value : [typeof d.value === 'number' ? d.value : 0];
}

/** The first dimension of a number. */
export function numValue(d: NumParam): number {
	return numValues(d)[0] ?? 0;
}
