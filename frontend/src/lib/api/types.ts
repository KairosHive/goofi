/** Wire shapes for parameter descriptors, assembled from the generated Rust types. */
import type { Mode, ParamBase, ParamKind, VideoQuality } from './generated';

export type { ParamBase, ParamShow, VideoQuality } from './generated';

/** The one active source of a param's value. */
export type ParamMode = Mode;

/** Every member once: the record is checked against the generated union in both directions. */
const keys = <K extends string>(all: Record<K, 0>): readonly K[] => Object.keys(all) as K[];
export const PARAM_MODES = keys<ParamMode>({ constant: 0, expression: 0, reference: 0 });
export const VIDEO_QUALITIES = keys<VideoQuality>({ small: 0, high: 0, very_high: 0 });

/** What `node param edit` takes beside a value: any subset, and a text given implies its mode. */
export interface SourcePatch {
	mode?: ParamMode;
	expression?: string;
	reference?: string;
	triggers?: boolean;
}

type Kind<T extends ParamKind['type']> = ParamBase & Extract<ParamKind, { type: T }>;
export type FloatParam = Kind<'float'>;
export type IntParam = Kind<'int'>;
export type BoolParam = Kind<'bool'>;
export type StringParam = Kind<'string'>;
export type PulseParam = Kind<'pulse'>;

/** A descriptor for a param whose node type is absent from the catalog: no bounds, no options. */
export type UnknownParam = ParamBase & { type: 'unknown'; value: unknown };

export type ParamDescriptor = (ParamBase & ParamKind) | UnknownParam;
