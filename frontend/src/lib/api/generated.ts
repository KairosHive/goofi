// GENERATED from the Rust types in goofi-core, goofi-graph and goofi-bridge — do not edit by
// hand. The document, its deltas, a variable and a param descriptor are each declared once,
// in Rust; a field that is not there is a type error here. Regenerate by running
// `cargo test -p goofi-tests contracts::`, which rewrites this file when it drifts.

export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
export type Literal = number | string | boolean | Literal[];
export type Mode = "constant" | "expression";
export type ParamEntry = { value?: Literal, mode?: Mode, expression?: string, triggers?: boolean, };
export type VideoQuality = "small" | "high" | "very_high";
export type RecordedOutput = { slot: string, quality: VideoQuality, };
export type NodeRecord = { type: string, name: string, pos: [number, number], 
/**
 * The scope this record is a member of; absent is ROOT.
 */
scope?: string, params: { [key in string]: { [key in string]: ParamEntry } }, viewers?: JsonValue, baseline?: JsonValue, record: Array<RecordedOutput>, };
export type Link = { node_out: string, slot_out: string, node_in: string, slot_in: string, };
export type ControlKind = "knob" | "slider" | "number" | "text" | "toggle" | "dropdown" | "paint" | "vector" | "color";
export type Control = { kind: ControlKind, min?: number, max?: number, step?: number, options: Array<string>, 
/**
 * A paint pad's side in texels; absent is 128.
 */
resolution?: number, x: number, y: number, w: number, h: number, };
export type Lock = { config: boolean, value: boolean, };
export type VariableRecord = { value?: Literal, control?: Control, 
/**
 * The expression the manager computes it by, bare or in Python; absent is set by hand.
 */
expression?: string, lock?: Lock, };
export type Midi = { port: string, };
export type Group = { lock: Lock, midi?: Midi, machine?: string, };
export type AttributeKind = { "type": "num", vmin: number, vmax: number, int: boolean, color: boolean, } | { "type": "bool" } | { "type": "string", options?: Array<string>, };
export type Attribute = { default: Literal, kind: AttributeKind, };
export type State = { pos: [number, number], values?: Record<string, Literal>, };
export type Curve = "step" | "linear" | "in" | "out" | "in_out" | "smooth";
export type Policy = "fifo" | "lifo" | "all";
export type Seconds = number | string;
export type Trigger = { "kind": "manual" } | { "kind": "after", seconds: Seconds, weight: number, } | { "kind": "when", expression: string, weight: number, } | { "kind": "meet", policy: Policy, } | { "kind": "alone" };
export type Transition = { 
/**
 * A state name, or `*` for any state.
 */
from: string, to: string, triggers?: Array<Trigger>, 
/**
 * Seconds; 0 is instant.
 */
duration: number, curve: Curve, };
export type Playhead = { color: string, start: string, };
export type Machine = { attributes: { [key in string]: Attribute }, states: { [key in string]: State }, transitions: { [key in string]: Transition }, playheads: { [key in string]: Playhead }, 
/**
 * What the random draws start from; absent is 0.
 */
seed?: number, };
export type PatchDoc = { 
/**
 * Keyed by uid spelling; a key that is not one is reminted on the way in.
 */
nodes: { [key in string]: NodeRecord }, 
/**
 * Keyed `out.slot>in.slot`, which is derived from the link and never read back.
 */
links: { [key in string]: Link }, variables: { [key in string]: VariableRecord }, variable_groups: { [key in string]: Group }, machines: { [key in string]: Machine }, arrangement?: JsonValue, };
export type Archive = { version: number, goofi: string, patch: PatchDoc, viewpoint?: JsonValue, };
export type Op = { "op": "put", path: Array<string>, value: JsonValue, } | { "op": "del", path: Array<string>, };
export type ParamShow = { group: string, name: string, any_of: Array<string>, };
export type ParamRole = { "as": "count", section: string, } | { "as": "member", section: string, base: string, slot: number | null, } | { "as": "feed", slot: string, group: string, };
export type ParamBase = { doc: string | null, 
/**
 * What the declaration says this param is worth untouched; `None` for a pulse.
 */
default: Literal | null, 
/**
 * The index of the param's section inside its group; the inspector draws a line between two.
 */
section: number, 
/**
 * The inspector shows the param only while this holds; `None` shows it always.
 */
show: ParamShow | null, 
/**
 * The part the param plays in a declared section; `None` for a row of its own.
 */
role: ParamRole | null, 
/**
 * True when the node declared a refresh method for this param.
 */
refreshable: boolean, mode: Mode, expression: string | null, 
/**
 * When true, an arrival that changes the value wakes the node's `process()`.
 */
triggers: boolean, 
/**
 * The active source's bind, compile or arrival error.
 */
error: string | null, };
export type ParamKind = { "type": "num", value: number | number[], vmin: number, vmax: number, int: boolean, options: Array<number>, color: boolean, } | { "type": "bool", value: boolean, } | { "type": "string", value: string, options: Array<string> | null, } | { "type": "pulse", value: null, };
