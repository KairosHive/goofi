import { describe, it, expect, beforeEach } from 'vitest';
import { ControlClient } from './control';

// The tests run in a node env (no browser sessionStorage) — provide a Map-backed stand-in so the
// per-tab session id can be minted + read back deterministically.
beforeEach(() => {
	const store = new Map<string, string>();
	const g = globalThis as unknown as { sessionStorage: Storage; location: { protocol: string; host: string } };
	g.sessionStorage = {
		getItem: (k: string) => store.get(k) ?? null,
		setItem: (k: string, v: string) => void store.set(k, v),
		removeItem: (k: string) => void store.delete(k),
		clear: () => store.clear(),
		key: () => null,
		length: 0
	} as Storage;
	// The constructor reads `location.protocol` to derive ws:// vs wss:// (even when a url is given).
	g.location = { protocol: 'http:', host: 'test' };
});

describe('ControlClient per-tab command session', () => {
	it('mints a non-empty session id, stable within a tab (not the shared default)', () => {
		// The manager keeps undo and redo per actor: a client with no actor would share `default`.
		// A url is passed so the constructor does not read `location`.
		const a = new ControlClient('ws://test/control');
		expect(a.actor).toBeTruthy();
		expect(a.actor).not.toBe('default');
		// Same tab (shared sessionStorage) → a second client reuses the SAME id, so the tag survives
		// a WS reconnect. Cross-TAB distinctness is inherent to sessionStorage (one store per tab);
		// a two-tab isolation check belongs in e2e, not a single env.
		const b = new ControlClient('ws://test/control');
		expect(b.actor).toBe(a.actor);
	});
});
