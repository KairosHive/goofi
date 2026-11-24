import { describe, expect, it } from 'vitest';
import { wsUrl } from './wsUrl';

describe('wsUrl', () => {
	it('builds the /data/<node>/<slot> WS URL', () => {
		expect(wsUrl(['data', 'osc0', 'out'], { protocol: 'http:', host: 'localhost:8000' })).toBe('ws://localhost:8000/data/osc0/out');
	});

	it('url-encodes node and slot segments', () => {
		expect(wsUrl(['data', 'sub0::psd0', 'a/b'], { protocol: 'https:', host: 'h' })).toBe('wss://h/data/sub0%3A%3Apsd0/a%2Fb');
	});
});
