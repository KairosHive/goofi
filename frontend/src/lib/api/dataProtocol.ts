/** What crosses between the main thread and `dataWorker.ts`, both ways, and how a head is made. */
import type { ArrayData, DataFrame, DataType } from '$lib/codec/decode';
import type { ViewSpec } from '$lib/viewers/module';
import { summaryOf, type ViewSummary } from '$lib/viewers/viewMeta';
import type { DrawBox, DrawnState } from '$lib/viewers/drawing';
import type { Hover, ProbeBox } from '$lib/viewers/hover';
import type { ViewerKind } from '$lib/viewers/registry';
import type { SettingsMap } from '$lib/viewers/module';
import type { View } from 'plotluck';

/** What every frame says about itself, for a thread that does not hold the frame. */
export interface FrameHead {
	dtype: DataType;
	meta: Record<string, unknown>;
	/** An array's summary in the node's units, with the wire array's own shape and element count. */
	array?: ViewSummary & { wireShape: number[]; length: number };
	text?: string;
}

/** The routing key of a (node, slot) stream; one stream per slot, so a viewer `kind` is not in it. */
export function streamKey(node: string, slot: string): string {
	return `${node} ${slot}`;
}

/** A frame's head: what a thread that holds no frame is told of it. */
export function headOf(f: DataFrame): FrameHead {
	const head: FrameHead = { dtype: f.dtype, meta: f.meta };
	if (f.dtype === 'ARRAY') {
		const a = f.data as ArrayData;
		head.array = { ...summaryOf(a, f.meta), wireShape: a.shape, length: a.values.length };
	} else if (f.dtype === 'STRING') head.text = f.data as string;
	return head;
}

export type ToWorker =
	| { op: 'rate'; fps: number }
	| { op: 'sub'; node: string; slot: string; frames: boolean }
	| { op: 'spec'; node: string; slot: string; specs: ViewSpec[]; frames: boolean }
	| { op: 'unsub'; node: string; slot: string }
	| { op: 'surface'; id: number; canvas: OffscreenCanvas }
	| { op: 'view'; id: number; view: View }
	| { op: 'dispose'; id: number }
	| { op: 'drawing'; id: number; surface: number; kind: ViewerKind; variant: string }
	| { op: 'drop'; id: number }
	| { op: 'place'; id: number; x: number; y: number; w: number; h: number; z: number }
	| { op: 'background'; id: number; hex: string }
	| { op: 'settings'; id: number; settings: SettingsMap; box: DrawBox }
	| { op: 'attach'; id: number; node: string; slot: string }
	| { op: 'detach'; id: number; keep: boolean }
	| { op: 'pointer'; id: number; at: { x: number; y: number; box: ProbeBox } | null }
	| { op: 'drag'; id: number; dx: number; dy: number; box: ProbeBox };

/** What one flush tells the main thread of a stream: its newest frame where a reader there asked,
 * else its head, or the stamps alone where a held frame only moved in time. */
export interface StreamNews {
	node: string;
	slot: string;
	frame?: DataFrame;
	head?: FrameHead;
	stamps?: Record<string, unknown>;
}

export type ToMain =
	| { batch: StreamNews[] }
	| { drawn: { id: number } & DrawnState }
	| { hover: { id: number; hover: Hover | null } }
	| { surface: { id: number; ok: boolean } }
	| { stats: { paints: number; streams: [string, string, number, number][] } };
