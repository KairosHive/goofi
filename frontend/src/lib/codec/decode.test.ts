import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { decodeData, type ArrayData, type DataFrame } from './decode';

/**
 * Cross-language codec parity. The fixture (tests/codec_golden.json) is written by the canonical
 * encoder (`goofi-codec`, the Rust source of truth, through `cargo test -p goofi-tests
 * codec_golden::`): hex-encoded frames and their expected decodings. Decoding it here proves the
 * hand-mirrored TS port stays in lock-step; any drift in tag, header or body fails this test.
 */
const GOLDEN = JSON.parse(
	readFileSync(fileURLToPath(new URL('../../../../tests/codec_golden.json', import.meta.url)), 'utf-8')
) as { entries: { name: string; hex: string; decoded: Decoded }[] };

interface Decoded {
	dtype: 'ARRAY' | 'STRING' | 'TABLE';
	meta: Record<string, unknown>;
	arrayDtype?: string;
	shape?: number[];
	/** JSON has no NaN or infinity: the fixture spells those as the strings `Number` reads. */
	values?: (number | string)[];
	value?: string;
	entries?: Record<string, Decoded>;
}

function hexToBytes(hex: string): Uint8Array {
	const out = new Uint8Array(hex.length / 2);
	for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
	return out;
}

function checkDecoded(frame: DataFrame, exp: Decoded): void {
	expect(frame.dtype).toBe(exp.dtype);
	expect(frame.meta).toEqual(exp.meta);
	if (exp.dtype === 'ARRAY') {
		const a = frame.data as ArrayData;
		expect(a.dtype).toBe(exp.arrayDtype);
		expect(a.shape).toEqual(exp.shape);
		// Number() coerces BigInt64Array (i8/u8) elements; all fixture values < 2^53.
		expect(Array.from(a.values, (v) => Number(v))).toEqual(exp.values!.map(Number));
	} else if (exp.dtype === 'STRING') {
		expect(frame.data).toBe(exp.value);
	} else {
		const tbl = frame.data as Record<string, DataFrame>;
		expect(Object.keys(tbl).sort()).toEqual(Object.keys(exp.entries!).sort());
		for (const [k, sub] of Object.entries(exp.entries!)) checkDecoded(tbl[k], sub);
	}
}

describe('codec golden parity (rust encode → ts decode)', () => {
	for (const entry of GOLDEN.entries) {
		it(`decodes ${entry.name} as the encoder meant it`, () => {
			checkDecoded(decodeData(hexToBytes(entry.hex)), entry.decoded);
		});
	}
});

describe('codec golden parity on an engine without Float16Array', () => {
	beforeAll(() => vi.stubGlobal('Float16Array', undefined));
	afterAll(() => vi.unstubAllGlobals());

	for (const entry of GOLDEN.entries.filter((e) => e.name.includes('f16'))) {
		it(`widens ${entry.name} bit by bit`, () => {
			checkDecoded(decodeData(hexToBytes(entry.hex)), entry.decoded);
		});
	}
});

/* The data worker drops a frame whose decode throws and keeps the slot. A frame that is cut or
 * that is not a frame must throw, or the viewer would draw what the node did not send. */
describe('a frame the decoder cannot read is refused', () => {
	const golden = (name: string): Uint8Array => hexToBytes(GOLDEN.entries.find((e) => e.name === name)!.hex);
	/** `f32_1d` with the three characters of its sample dtype replaced. */
	const withSampleDtype = (dtype: string): Uint8Array => {
		const bytes = golden('f32_1d');
		const metaLen = new DataView(bytes.buffer).getUint32(5, true);
		const dtypeAt = Math.ceil((13 + metaLen) / 4) * 4 + 2;
		expect(new TextDecoder().decode(bytes.slice(dtypeAt, dtypeAt + 3))).toBe('<f4');
		bytes.set(new TextEncoder().encode(dtype), dtypeAt);
		return bytes;
	};

	for (const entry of GOLDEN.entries) {
		it(`refuses ${entry.name} cut short at every length`, () => {
			const whole = hexToBytes(entry.hex);
			const decoded: number[] = [];
			for (let n = 0; n < whole.length; n++) {
				try {
					decodeData(whole.slice(0, n));
					decoded.push(n);
				} catch {
					// refused
				}
			}
			expect(decoded, 'the lengths that decoded').toEqual([]);
		});
	}

	it('refuses bytes that do not start with the magic', () => {
		const bytes = golden('f32_1d');
		bytes[0] ^= 0xff;
		expect(() => decodeData(bytes)).toThrow('bad magic');
	});

	it('refuses a tag it does not know and a kind the browser does not draw', () => {
		const bytes = golden('string');
		bytes[4] = 9;
		expect(() => decodeData(bytes)).toThrow('Unknown dtype tag 9');
		bytes[4] = 3;
		expect(() => decodeData(bytes)).toThrow('A TEXTURE frame does not decode in the browser');
	});

	it('refuses samples in a byte order or a width the wire does not carry', () => {
		expect(() => decodeData(withSampleDtype('>f4'))).toThrow('Big-endian arrays unsupported: >f4');
		expect(() => decodeData(withSampleDtype('<i4'))).toThrow('Unsupported numpy dtype: <i4');
		expect(() => decodeData(withSampleDtype('<f8'))).toThrow('Unsupported numpy dtype: <f8');
	});
});
