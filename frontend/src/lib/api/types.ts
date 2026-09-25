/** Wire shapes for parameter descriptors as the bridge emits them. */

/** The one active source of a param's value. */
export const PARAM_MODES = ['constant', 'expression', 'reference'] as const;
export type ParamMode = (typeof PARAM_MODES)[number];

/** What `node param edit` takes beside a value: any subset, and a text given implies its mode. */
export interface SourcePatch {
	mode?: ParamMode;
	expression?: string;
	reference?: string;
	triggers?: boolean;
}

export interface BaseParam {
	value: unknown;
	/** What the declaration says this param is worth untouched; null for a pulse. */
	default: unknown;
	doc: string | null;
	/** True when the node declared a refresh method for this param. */
	refreshable: boolean;
	mode: ParamMode;
	/** The retained expression text, whatever the mode; null when there is none. */
	expression: string | null;
	/** The retained `node.slot`, whatever the mode; null when there is none. */
	reference: string | null;
	/** When true, an arrival that changes the value wakes the node's `process()`. */
	triggers: boolean;
	/** The active source's bind, compile or arrival error, or null. */
	error: string | null;
	/** The index of the param's section inside its group; the inspector draws a line between two. */
	section: number;
	/** The inspector shows the param only while this holds; null shows it always. */
	show: ParamShow | null;
}

/** Holds while the param `group.name` has one of `any_of`, compared as text. */
export interface ParamShow {
	group: string;
	name: string;
	any_of: string[];
}

export interface FloatParam extends BaseParam {
	type: 'float';
	value: number;
	vmin: number;
	vmax: number;
}

export interface IntParam extends BaseParam {
	type: 'int';
	options?: number[];
	value: number;
	vmin: number;
	vmax: number;
}

export interface BoolParam extends BaseParam {
	type: 'bool';
	value: boolean;
}

export interface StringParam extends BaseParam {
	type: 'string';
	value: string;
	options: string[] | null;
}

/** A request rather than a value: it holds none, and firing it is the whole edit. */
export interface PulseParam extends BaseParam {
	type: 'pulse';
	value: null;
}

export interface UnknownParam extends BaseParam {
	type: 'unknown';
}

export type ParamDescriptor =
	| FloatParam
	| IntParam
	| BoolParam
	| StringParam
	| PulseParam
	| UnknownParam;

export const VIDEO_QUALITIES = ['small', 'high', 'very_high'] as const;
export type VideoQuality = (typeof VIDEO_QUALITIES)[number];
