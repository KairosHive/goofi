// The browser's reader of a drawing's byte code; `goofi_core::drawing` owns the format.
// Coordinates, width and softness are u16, 64 to a unit of the 1000 span.

export const SPAN = 1000;
const Q = 64000;

export type Ink = string | 'erase';
type Seg = { tag: 0 | 1 | 2 | 3; pts: number[] };
export type DrawOp =
	| { op: 'clear' }
	| { op: 'stroke'; ink: Ink; width: number; soft: number; cap: CanvasLineCap; dash: number[]; path: Seg[] }
	| { op: 'fill'; ink: Ink; path: Seg[] };

const CAPS: CanvasLineCap[] = ['round', 'butt', 'square'];
/** Dash patterns in multiples of the width, as the Rust rasterizer reads them. */
const DASHES = [[], [3, 2], [1, 2]];

/** The ops in a variable's value; an empty or broken value is a blank sheet. */
export function decode(value: string): DrawOp[] {
	let bytes: Uint8Array;
	try {
		bytes = Uint8Array.from(atob(value.trim()), (c) => c.charCodeAt(0));
	} catch {
		return [];
	}
	let at = 0;
	const byte = () => {
		if (at >= bytes.length) throw new Error('short');
		return bytes[at++];
	};
	const varint = () => {
		let v = 0;
		for (let mul = 1; ; mul *= 128) {
			const b = byte();
			v += (b & 0x7f) * mul;
			if (b < 0x80) return v;
		}
	};
	const ink = (): [number, Ink] => {
		const flags = byte();
		if (flags & 0x10) return [flags, 'erase'];
		const [r, g, b, a] = [byte(), byte(), byte(), byte()];
		return [flags, `rgba(${r},${g},${b},${a / 255})`];
	};
	const path = (): Seg[] => {
		const n = varint();
		const p = [0, 0];
		const segs: Seg[] = [];
		for (let i = 0; i < n; i++) {
			const tag = (varint() % 4) as Seg['tag'];
			const pts: number[] = [];
			for (let k = 0; k < [1, 1, 3, 0][tag] * 2; k++) {
				const z = varint();
				p[k % 2] += z % 2 ? -(z + 1) / 2 : z / 2;
				pts.push(p[k % 2]);
			}
			segs.push({ tag, pts });
		}
		return segs;
	};
	const ops: DrawOp[] = [];
	try {
		while (at < bytes.length) {
			const code = varint() % 4;
			if (code === 0) ops.push({ op: 'clear' });
			else if (code === 1) {
				const [flags, i] = ink();
				const width = varint();
				const soft = varint();
				ops.push({ op: 'stroke', ink: i, width, soft, cap: CAPS[flags & 3], dash: DASHES[(flags >> 2) & 3], path: path() });
			} else if (code === 2) ops.push({ op: 'fill', ink: ink()[1], path: path() });
			else break;
		}
	} catch {
		// A truncated value paints what it holds up to the break.
	}
	return ops;
}

function trace(ctx: CanvasRenderingContext2D, path: Seg[], k: number): void {
	ctx.beginPath();
	for (const { tag, pts } of path) {
		const p = pts.map((v) => v * k);
		if (tag === 0) ctx.moveTo(p[0], p[1]);
		else if (tag === 1) ctx.lineTo(p[0], p[1]);
		else if (tag === 2) ctx.bezierCurveTo(p[0], p[1], p[2], p[3], p[4], p[5]);
		else ctx.closePath();
	}
}

/** Paint `ops` on a `size` square canvas, as `goofi_core::drawing::raster` does. */
export function paint(ctx: CanvasRenderingContext2D, ops: DrawOp[], size: number): void {
	const k = size / Q;
	ctx.clearRect(0, 0, size, size);
	for (const o of ops) {
		if (o.op === 'clear') {
			ctx.clearRect(0, 0, size, size);
			continue;
		}
		ctx.save();
		ctx.globalCompositeOperation = o.ink === 'erase' ? 'destination-out' : 'source-over';
		const colour = o.ink === 'erase' ? '#000' : o.ink;
		trace(ctx, o.path, k);
		if (o.op === 'fill') {
			ctx.fillStyle = colour;
			ctx.fill();
		} else {
			const w = Math.max(o.width * k, 0.5);
			ctx.strokeStyle = colour;
			ctx.lineWidth = w;
			ctx.lineCap = o.cap;
			ctx.lineJoin = 'round';
			ctx.setLineDash(o.dash.map((d) => d * w));
			// `filter` is what makes a soft stroke soft; where it is unsupported the edge stays hard.
			if (o.soft * k >= 0.5) ctx.filter = `blur(${o.soft * k}px)`;
			ctx.stroke();
		}
		ctx.restore();
	}
}

/** A hand stroke in the text form `control paint` takes. Points are `[x, y, ms since the last]`. */
export function strokeText(
	ink: Ink,
	width: number,
	soft: number,
	points: [number, number, number][],
	dt: number
): string {
	const n = (v: number) => String(Math.round(Math.min(Math.max(v, 0), SPAN) * 10) / 10);
	const t = (ms: number) => (ms > 0 ? `+${Math.round(ms)} ` : '');
	const segs = points.map(([x, y, ms], i) => `${t(ms)}${i ? 'L' : 'M'} ${n(x)} ${n(y)}`);
	// A tap is a dot: a zero-length line with round caps still leaves a mark.
	if (points.length === 1) segs.push(`L ${n(points[0][0])} ${n(points[0][1])}`);
	return `${t(dt)}stroke ${ink} width ${n(width)} soft ${n(soft)} : ${segs.join(' ')}`;
}
