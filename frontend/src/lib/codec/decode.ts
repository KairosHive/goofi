/** Decode side of the GOOF wire format, whose source of truth is backend/goofi-codec/src/lib.rs. */
import { decode as msgpackDecode } from '@msgpack/msgpack';

export type DataType = 'ARRAY' | 'STRING' | 'TABLE';

const DTYPE_TAG: Record<number, DataType> = {
	0: 'ARRAY',
	1: 'STRING',
	2: 'TABLE'
};

/** A decoded Data frame. */
export interface DataFrame {
	dtype: DataType;
	data: ArrayData | string | Record<string, DataFrame>;
	meta: Record<string, unknown>;
}

export interface ArrayData {
	/** numpy dtype string, e.g. '<f4', '<i8', '|u1'. */
	dtype: string;
	shape: number[];
	/** Linear element buffer in row-major order. */
	values: Float32Array | Uint8Array;
}

const MAGIC = new Uint8Array([0x47, 0x4f, 0x4f, 0x46]); // 'GOOF'

const decoder = new TextDecoder('utf-8');

function checkMagic(view: DataView, off: number): void {
	if (
		view.getUint8(off) !== MAGIC[0] ||
		view.getUint8(off + 1) !== MAGIC[1] ||
		view.getUint8(off + 2) !== MAGIC[2] ||
		view.getUint8(off + 3) !== MAGIC[3]
	) {
		throw new Error('Invalid Data frame: bad magic');
	}
}

/** The tag of a frame that carries a held frame's per-emit stamps (`time`, `index`, `ufreq`) and
 * no body: the reducer sends it in place of a frame that says what the last one said. */
const STAMPS_TAG = 4;

/** The stamps of a stamps frame, or null for a frame with data in it. */
export function decodeStamps(buf: ArrayBuffer): Record<string, unknown> | null {
	const view = new DataView(buf);
	checkMagic(view, 0);
	if (view.getUint8(5) !== STAMPS_TAG) return null;
	const metaLen = view.getUint32(6, true);
	const m = metaLen > 0 ? msgpackDecode(new Uint8Array(buf, 14, metaLen)) : {};
	return m && typeof m === 'object' ? (m as Record<string, unknown>) : {};
}

/** Decode an encoded GOOF buffer into a DataFrame. */
export function decodeData(buf: ArrayBuffer | Uint8Array): DataFrame {
	const bytes = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
	const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
	return decodeInto(view, 0);
}

function decodeInto(view: DataView, off: number): DataFrame {
	checkMagic(view, off);
	const version = view.getUint8(off + 4);
	if (version !== 2) throw new Error(`Unsupported GOOF version ${version}`);
	const dtypeTag = view.getUint8(off + 5);
	const dtype = DTYPE_TAG[dtypeTag];
	if (!dtype) throw new Error(`Unknown dtype tag ${dtypeTag}`);
	const metaLen = view.getUint32(off + 6, true);
	const bodyLen = view.getUint32(off + 10, true);
	const headerEnd = off + 14;
	const meta =
		metaLen > 0
			? ((): Record<string, unknown> => {
					const slice = new Uint8Array(view.buffer, view.byteOffset + headerEnd, metaLen);
					const m = msgpackDecode(slice);
					return (m && typeof m === 'object' ? (m as Record<string, unknown>) : {}) ?? {};
				})()
			: {};
	const bodyStart = headerEnd + metaLen;
	const bodyEnd = bodyStart + bodyLen;
	let data: ArrayData | string | Record<string, DataFrame>;
	if (dtype === 'ARRAY') {
		const arr = decodeArray(view, bodyStart, bodyEnd);
		// A half-float hop leaves here as f32, body and meta, so nothing downstream learns a
		// second float width.
		if (arr.dtype === '<f2') {
			arr.dtype = '<f4';
			meta['dtype'] = 'float32';
		}
		data = arr;
	} else if (dtype === 'STRING') {
		data = decoder.decode(new Uint8Array(view.buffer, view.byteOffset + bodyStart, bodyLen));
	} else {
		data = decodeTable(view, bodyStart);
	}
	return { dtype, data, meta };
}

function decodeArray(view: DataView, start: number, end: number): ArrayData {
	let off = start;
	const ndim = view.getUint8(off);
	off += 1;
	const dtypeStrLen = view.getUint8(off);
	off += 1;
	const dtypeStr = decoder.decode(new Uint8Array(view.buffer, view.byteOffset + off, dtypeStrLen));
	off += dtypeStrLen;
	const shape: number[] = [];
	for (let i = 0; i < ndim; i++) {
		shape.push(view.getUint32(off, true));
		off += 4;
	}
	const nBytes = end - off;
	const values = readTypedArray(dtypeStr, view.buffer, view.byteOffset + off, nBytes);
	return { dtype: dtypeStr, shape, values };
}

function decodeTable(view: DataView, start: number): Record<string, DataFrame> {
	let off = start;
	const n = view.getUint32(off, true);
	off += 4;
	const out: Record<string, DataFrame> = {};
	for (let i = 0; i < n; i++) {
		const keyLen = view.getUint16(off, true);
		off += 2;
		const key = decoder.decode(new Uint8Array(view.buffer, view.byteOffset + off, keyLen));
		off += keyLen;
		const valueLen = view.getUint32(off, true);
		off += 4;
		out[key] = decodeInto(view, off);
		off += valueLen;
	}
	return out;
}

function readTypedArray(
	dtypeStr: string,
	buffer: ArrayBufferLike,
	byteOffset: number,
	nBytes: number
): Float32Array | Uint8Array {
	const bo = dtypeStr.charAt(0);
	if (bo === '>') {
		throw new Error(`Big-endian arrays unsupported: ${dtypeStr}`);
	}
	const tail = bo === '<' || bo === '=' || bo === '|' ? dtypeStr.slice(1) : dtypeStr;
	const kind = tail.charAt(0);
	const itemsize = parseInt(tail.slice(1), 10);
	const count = nBytes / itemsize;
	// The reducer's 8-bit hop for an image viewer: bytes need no alignment, so the view sits on the
	// message buffer itself.
	if (kind + itemsize === 'u1') return new Uint8Array(buffer, byteOffset, count);
	// Slice into a fresh buffer: an f32 view must be 4-byte aligned, which the body offset is not.
	const slice = buffer.slice(byteOffset, byteOffset + nBytes);
	if (kind + itemsize === 'f4') return new Float32Array(slice, 0, count);
	// The reducer's half-float hop for a line viewer, widened here so nothing downstream changes.
	if (kind + itemsize === 'f2') return expandHalf(slice, count);
	throw new Error(`Unsupported numpy dtype: ${dtypeStr} (the wire is f32, and u8 or f16 on the viewer hop)`);
}

/** `count` half floats at the start of `buf`, widened to f32 by the engine where it has
 * `Float16Array` and bit by bit where it does not. */
function expandHalf(buf: ArrayBufferLike, count: number): Float32Array {
	const F16 = (globalThis as { Float16Array?: new (b: ArrayBufferLike, o: number, n: number) => ArrayLike<number> })
		.Float16Array;
	if (F16) return new Float32Array(new F16(buf, 0, count));
	const bits = new Uint16Array(buf, 0, count);
	const out = new Float32Array(count);
	for (let i = 0; i < count; i++) out[i] = halfToFloat(bits[i]);
	return out;
}

/** One IEEE 754 half float, from its bits. */
function halfToFloat(bits: number): number {
	const sign = bits & 0x8000 ? -1 : 1;
	const exp = (bits >> 10) & 0x1f;
	const frac = bits & 0x3ff;
	if (exp === 0) return sign * frac * 2 ** -24;
	if (exp === 0x1f) return frac ? NaN : sign * Infinity;
	return sign * (1 + frac / 1024) * 2 ** (exp - 15);
}

export function isArrayFrame(f: DataFrame): f is DataFrame & { data: ArrayData } {
	return f.dtype === 'ARRAY';
}
export function isStringFrame(f: DataFrame): f is DataFrame & { data: string } {
	return f.dtype === 'STRING';
}
export function isTableFrame(f: DataFrame): f is DataFrame & { data: Record<string, DataFrame> } {
	return f.dtype === 'TABLE';
}
