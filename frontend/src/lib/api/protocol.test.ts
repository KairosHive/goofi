import { afterEach, describe, expect, it, vi } from 'vitest';
import { ControlClient, PROTOCOL_VERSION } from './control';

afterEach(() => vi.unstubAllGlobals());

/** Whether a client latches a mismatch from a `hello` that reports `version`. */
function mismatchOn(version: unknown): boolean {
	let deliver: (e: { data: string }) => void = () => {};
	vi.stubGlobal('WebSocket', class {
		addEventListener(type: string, cb: (e: { data: string }) => void): void {
			if (type === 'message') deliver = cb;
		}
	});
	const client = new ControlClient('ws://test/control');
	client.connect();
	deliver({ data: JSON.stringify({ event: 'hello', payload: { protocol_version: version } }) });
	let latched = false;
	client.onProtocolMismatch(() => (latched = true));
	return latched;
}

describe('control protocol version', () => {
	it('accepts an exact match', () => {
		expect(mismatchOn(PROTOCOL_VERSION)).toBe(false);
	});

	it('rejects a newer or older backend version', () => {
		expect(mismatchOn(PROTOCOL_VERSION + 1)).toBe(true);
		expect(mismatchOn(PROTOCOL_VERSION - 1)).toBe(true);
	});

	it('rejects an absent/non-numeric version (backend predates the field = skew)', () => {
		expect(mismatchOn(undefined)).toBe(true);
		expect(mismatchOn(null)).toBe(true);
		expect(mismatchOn('1')).toBe(true);
	});
});
