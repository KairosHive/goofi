// GENERATED from backend/goofi-bridge/src/vocab.rs — do not edit by hand.
// Panel types and viewer kinds are declared ONCE, in the manager: naming one that is not
// in the table is a type error here and a teachable refusal there. The BEHAVIOUR keyed off
// a word stays client-side — which component renders a panel type (`panels/register.ts`),
// and whether a particular array draws (`viewers/registry.ts`). Regenerate by running
// `cargo test -p goofi-bridge`, which rewrites this file when it drifts.
import type { IconName } from '$lib/ui';

export type PanelTypeId =
	| 'empty'
	| 'node-editor'
	| 'inspector'
	| 'viewer'
	| 'console'
	| 'variables'
	| 'control'
	| 'machine'
	| 'agent'
	| 'recorder';

/** The panel type a brand-new tab starts with. */
export const DEFAULT_PANEL_TYPE = 'node-editor';
/** The placeholder a split births, whose in-panel grid chooses its content. */
export const EMPTY_PANEL_TYPE = 'empty';

export type ViewerKind =
	| 'line'
	| 'image'
	| 'brain'
	| 'string'
	| 'table';

export interface PanelTypeInfo {
	readonly id: PanelTypeId;
	readonly title: string;
	readonly icon: IconName;
	readonly doc: string;
}

/** The slot kinds, as the manager names them. */
export type SlotDtype = 'ARRAY' | 'STRING' | 'TABLE' | 'AUDIO' | 'TEXTURE';

export interface ViewerKindInfo {
	readonly id: ViewerKind;
	/** The slot dtype this kind serves. A non-ARRAY dtype PINS its kind: a STRING slot is
	 * always drawn by the string viewer, whatever kind was stored. */
	readonly dtype: SlotDtype;
	/** The dimension range the component actually renders — null for a pinned kind. */
	readonly draws: readonly [number, number] | null;
	/** The dimension range its ViewSpec declares compatible: equal or wider than `draws`,
	 * so a frame the viewer only summarises still arrives reduced. */
	readonly accepts: readonly [number, number] | null;
	readonly doc: string;
}

export const PANEL_TYPES: readonly PanelTypeInfo[] = [
	{ id: 'empty', title: 'Empty', icon: 'square-dashed', doc: 'a placeholder with no content yet — what a fresh split births' },
	{ id: 'node-editor', title: 'Node Editor', icon: 'workflow', doc: 'the patch canvas — nodes, wires and sub-patches' },
	{ id: 'inspector', title: 'Inspector', icon: 'sliders-horizontal', doc: 'the parameters, metadata and errors of one node' },
	{ id: 'viewer', title: 'Viewer', icon: 'activity', doc: 'live frames from one output slot, drawn by `state.kind`' },
	{ id: 'console', title: 'Console', icon: 'terminal', doc: 'the patch log; a bound node filters it to that node' },
	{ id: 'variables', title: 'Variables', icon: 'globe', doc: 'the patch variables, which any expression can read' },
	{ id: 'control', title: 'Control', icon: 'sliders-vertical', doc: 'knobs, sliders and text widgets over one group of variables' },
	{ id: 'machine', title: 'State machine', icon: 'waypoints', doc: 'one state machine: its states, transitions and playheads on a canvas' },
	{ id: 'agent', title: 'Agent', icon: 'bot', doc: 'a terminal on an agent harness, running in the patch workspace' },
	{ id: 'recorder', title: 'Recorder', icon: 'circle-dot', doc: 'capture the output slots of any node to disk, on one clock' },
];

export const VIEWER_KINDS: readonly ViewerKindInfo[] = [
	{ id: 'line', dtype: 'ARRAY', draws: [0, 2], accepts: [0, 3], doc: 'a time plot: one series (1-D) or one per channel (C, N); or, as a setting, the trajectory over pairs of rows' },
	{ id: 'image', dtype: 'ARRAY', draws: [2, 3], accepts: [2, 3], doc: 'a bitmap: (H, W), or (H, W, C) for 1–4 channels' },
	{ id: 'brain', dtype: 'ARRAY', draws: [1, 2], accepts: [1, 2], doc: 'a scalp map of one scalar per channel (1-D), or the connectivity between named channels (2-D) as a ring or in 3-D' },
	{ id: 'string', dtype: 'STRING', draws: null, accepts: null, doc: 'the text of a STRING slot' },
	{ id: 'table', dtype: 'TABLE', draws: null, accepts: null, doc: 'the rows of a TABLE slot' },
];

/** The closed vocabulary a node's `tags` come from — the palette facets by it. */
export const TAGS = ['input', 'output', 'generator', 'transform', 'analysis', 'control', 'image', 'text', 'midi', 'eeg', 'cardio', 'motion', 'music', 'ml', 'connectivity', 'simulation'] as const;

export type ControlKindId =
	| 'knob'
	| 'slider'
	| 'number'
	| 'text'
	| 'toggle'
	| 'dropdown'
	| 'paint'
	| 'vector'
	| 'color'
	| 'envelope';

export interface ControlKindInfo {
	readonly id: ControlKindId;
	/** What a widget of this kind draws: one number, a text, an image, a vector, a colour, an envelope, or any frame's truth. */
	readonly draws: 'number' | 'text' | 'image' | 'vector' | 'color' | 'envelope' | 'any';
	/** The box it is born in, in grid units. */
	readonly w: number;
	readonly h: number;
}

/** The widget kinds a control panel offers, in palette order. */
export const CONTROL_KINDS: readonly ControlKindInfo[] = [
	{ id: 'knob', draws: 'number', w: 4, h: 4 },
	{ id: 'slider', draws: 'number', w: 8, h: 2 },
	{ id: 'number', draws: 'number', w: 4, h: 2 },
	{ id: 'text', draws: 'text', w: 6, h: 3 },
	{ id: 'toggle', draws: 'any', w: 2, h: 2 },
	{ id: 'dropdown', draws: 'text', w: 6, h: 2 },
	{ id: 'paint', draws: 'image', w: 8, h: 8 },
	{ id: 'vector', draws: 'vector', w: 8, h: 2 },
	{ id: 'color', draws: 'color', w: 2, h: 2 },
	{ id: 'envelope', draws: 'envelope', w: 8, h: 4 },
];

/** How many columns a control panel's grid is, whatever its pixel width. */
export const CONTROL_COLUMNS = 16;

/** What `sm('state')` answers a machine's expression: the residents, their names from the
* longest resident to the newest, and the arrivals and departures of this instant. */
export const STATE_FIELDS = ['residents', 'playheads', 'arrivals', 'departures'] as const;
/** What `me` answers a machine's expression before the playhead's attributes. */
export const ME_FIELDS = ['name', 'state', 'arrived'] as const;
/** What a playhead's group holds before its attributes: `sm('playhead').<x>` in a param's expression. */
export const PLAYHEAD_FIELDS = ['state', 'progress'] as const;

export interface ExprArgInfo {
	readonly name: string;
	/** What it takes, in a word. */
	readonly kind: string;
	/** The default as the user would spell it; null is required. */
	readonly default: string | null;
	/** The closed set of strings it takes, where there is one. */
	readonly options: readonly string[];
	/** Keyword-only: spelled `name=value`. */
	readonly keyword: boolean;
	readonly doc: string;
}

export interface ExprBuiltinInfo {
	readonly name: string;
	readonly kind: 'function' | 'value' | 'namespace';
	/** Where it reads: every expression, a param's, or a machine's. */
	readonly scope: 'all' | 'node' | 'machine';
	readonly signature: string;
	readonly doc: string;
	/** What its one name argument picks from the live patch. */
	readonly pick: 'node' | 'machine' | null;
	readonly args: readonly ExprArgInfo[];
}

/** Every name an expression may use beyond Python's own, numpy and math: what the editor completes and documents. */
export const EXPR_BUILTINS: readonly ExprBuiltinInfo[] = [
	{ name: 'nd', kind: 'function', scope: 'all', signature: "nd('node')", doc: "A node's output: bare for a node with one output, `.out.<slot>` for one of several, `.params.<group>.<param>` for a param's current value.", pick: 'node', args: [
		{ name: 'name', kind: 'string', default: null, options: [], keyword: false, doc: "the node's display name" },
	] },
	{ name: 'sm', kind: 'function', scope: 'node', signature: "sm('playhead').element", doc: "A playhead of a state machine: `.state`, `.progress` or one of its attributes, as the machine publishes them.", pick: 'machine', args: [
		{ name: 'name', kind: 'string', default: null, options: [], keyword: false, doc: "a playhead's name" },
	] },
	{ name: 'sm', kind: 'function', scope: 'machine', signature: "sm('state' | 'playhead')", doc: "A state's census — `.residents`, `.playheads`, `.arrivals`, `.departures` — or another playhead's record, as `me` is.", pick: 'machine', args: [
		{ name: 'name', kind: 'string', default: null, options: [], keyword: false, doc: "a state's or a playhead's name in this machine" },
	] },
	{ name: 'me', kind: 'value', scope: 'node', signature: "me", doc: "This node: bare for its one output, `.out.<slot>` for one of several, `.params.<group>.<param>` for one of its params.", pick: null, args: [
	] },
	{ name: 'me', kind: 'value', scope: 'machine', signature: "me", doc: "The playhead this expression is read for: `.name`, `.state`, `.arrived` and one field per attribute.", pick: null, args: [
	] },
	{ name: 'vars', kind: 'namespace', scope: 'all', signature: "vars.group.element", doc: "The patch's variables: a control panel's widgets, a MIDI device's controls and the system's facts.", pick: null, args: [
	] },
	{ name: 't', kind: 'value', scope: 'all', signature: "t", doc: "Patch seconds: the patch clock at this evaluation.", pick: null, args: [
	] },
	{ name: 'np', kind: 'namespace', scope: 'all', signature: "np", doc: "numpy, whole. A frame arrives as a float32 array; a text as a str.", pick: null, args: [
	] },
	{ name: 'lfo', kind: 'function', scope: 'all', signature: "lfo(freq=1, src=t, vmin=…, vmax=…, fn='sin')", doc: "A periodic wave between `vmin` and `vmax`, one cycle per `1/freq` of `src`.", pick: null, args: [
		{ name: 'freq', kind: 'number', default: "1", options: [], keyword: true, doc: "cycles per second of `src`" },
		{ name: 'src', kind: 'number or array', default: "t", options: [], keyword: true, doc: "what the phase runs over; the patch time unless given" },
		{ name: 'vmin', kind: 'number', default: "the param's minimum", options: [], keyword: true, doc: "the value at the bottom of the cycle" },
		{ name: 'vmax', kind: 'number', default: "the param's maximum", options: [], keyword: true, doc: "the value at the top of the cycle" },
		{ name: 'fn', kind: 'string', default: "'sin'", options: ['sin', 'cos', 'tri', 'sqr', 'saw'], keyword: true, doc: "the waveform over one cycle" },
	] },
	{ name: 'nse', kind: 'function', scope: 'all', signature: "nse(freq=1, src=t, vmin=…, vmax=…)", doc: "Smooth noise between `vmin` and `vmax`, new every `1/freq` of `src`.", pick: null, args: [
		{ name: 'freq', kind: 'number', default: "1", options: [], keyword: true, doc: "cycles per second of `src`" },
		{ name: 'src', kind: 'number or array', default: "t", options: [], keyword: true, doc: "what the phase runs over; the patch time unless given" },
		{ name: 'vmin', kind: 'number', default: "the param's minimum", options: [], keyword: true, doc: "the value at the bottom of the cycle" },
		{ name: 'vmax', kind: 'number', default: "the param's maximum", options: [], keyword: true, doc: "the value at the top of the cycle" },
	] },
	{ name: 'qtz', kind: 'function', scope: 'all', signature: "qtz(v, scale='major', root='C', mode='voct')", doc: "The nearest degree of `scale` from `root`, in the unit `v` came in.", pick: null, args: [
		{ name: 'v', kind: 'number or array', default: null, options: [], keyword: false, doc: "the pitch to snap, elementwise over an array" },
		{ name: 'scale', kind: 'string', default: "'major'", options: ['chromatic', 'major', 'minor', 'dorian', 'phrygian', 'lydian', 'mixolydian', 'locrian', 'pentatonic_major', 'pentatonic_minor', 'blues', 'harmonic_minor', 'whole_tone', 'harmonic'], keyword: false, doc: "the scale whose degrees are kept" },
		{ name: 'root', kind: 'string or number', default: "'C'", options: ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'], keyword: false, doc: "a note name, or semitones above C" },
		{ name: 'mode', kind: 'string', default: "'voct'", options: ['voct', 'hz'], keyword: false, doc: "what `v` is: volts per octave with C4 at 0 V, or hertz" },
	] },
];
/** The names `from math import *` brings into every expression. */
export const EXPR_MATH = ['acos', 'acosh', 'asin', 'asinh', 'atan', 'atan2', 'atanh', 'ceil', 'comb', 'copysign', 'cos', 'cosh', 'degrees', 'dist', 'erf', 'erfc', 'exp', 'expm1', 'fabs', 'factorial', 'floor', 'fmod', 'frexp', 'fsum', 'gamma', 'gcd', 'hypot', 'isclose', 'isfinite', 'isinf', 'isnan', 'isqrt', 'lcm', 'ldexp', 'lgamma', 'log', 'log10', 'log1p', 'log2', 'modf', 'nextafter', 'perm', 'pow', 'prod', 'radians', 'remainder', 'sin', 'sinh', 'sqrt', 'tan', 'tanh', 'trunc', 'ulp'] as const;
export const EXPR_MATH_CONSTANTS = ['pi', 'e', 'tau', 'inf', 'nan'] as const;

/** The type a sub-patch facade wears in the document. It is not in the palette — grouping
* is what makes one. */
export const SCOPE_TYPE = 'SubPatch';

/** The one slot a boundary port carries. */
export const BOUNDARY_SLOT = 'value';

export interface BoundaryTypeInfo {
	readonly type: string;
	/** An `in` port FEEDS the sub-patch, so it wears an output and is a link's SOURCE. */
	readonly dir: 'in' | 'out';
	readonly dtype: SlotDtype;
}

/** The boundary port types: a port's direction and dtype ARE its type. */
export const BOUNDARY_TYPES: readonly BoundaryTypeInfo[] = [
	{ type: 'InArray', dir: 'in', dtype: 'ARRAY' },
	{ type: 'InString', dir: 'in', dtype: 'STRING' },
	{ type: 'InTable', dir: 'in', dtype: 'TABLE' },
	{ type: 'InAudio', dir: 'in', dtype: 'AUDIO' },
	{ type: 'InTexture', dir: 'in', dtype: 'TEXTURE' },
	{ type: 'OutArray', dir: 'out', dtype: 'ARRAY' },
	{ type: 'OutString', dir: 'out', dtype: 'STRING' },
	{ type: 'OutTable', dir: 'out', dtype: 'TABLE' },
	{ type: 'OutAudio', dir: 'out', dtype: 'AUDIO' },
	{ type: 'OutTexture', dir: 'out', dtype: 'TEXTURE' },
];

export const boundaryType = (type: string): BoundaryTypeInfo | undefined =>
	BOUNDARY_TYPES.find((b) => b.type === type);

/** Which output kind may feed which input kind — the manager's one link rule, projected. */
export const FEEDS: ReadonlySet<string> = new Set(['ARRAY>ARRAY', 'STRING>STRING', 'TABLE>TABLE', 'AUDIO>AUDIO', 'TEXTURE>TEXTURE']);
export const feeds = (out: SlotDtype, into: SlotDtype): boolean => FEEDS.has(`${out}>${into}`);
/** The kinds that live inside one engine: a node of that engine reads one as a plan edge. */
export const ENGINE_LOCAL: ReadonlySet<SlotDtype> = new Set(['AUDIO', 'TEXTURE']);

/** The kind a slot of each dtype opens with, before a viewer has stored one of its own. */
export const DEFAULT_KIND: Record<SlotDtype, ViewerKind> = {
	ARRAY: 'line',
	STRING: 'string',
	TABLE: 'table',
	AUDIO: 'line',
	TEXTURE: 'image',
};
