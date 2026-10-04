import { describe, it, expect, beforeEach } from 'vitest';
import { FakeControl } from '$lib/test/fakeControl';
import { seed, type DocSeed } from '$lib/test/docSeed';
import { GraphStore } from './graph.svelte';
import { history } from './history.svelte';

/** Seed a system variable the way the manager sends one. */
function seedSystemVariable(d: DocSeed, name: string, value: number): void {
	d.variable(name, { value, system: true });
}

// Variables are now server COMMAND ops (EditVariable / a Compound rename), undoable and validated
// server-side (invalid name / collision / protected-system reject the RPC — covered by the bridge +
// engine tests). This file pins the CLIENT's job: map each mutator to the right command op/payload
// and record an undoable step, propagating a server rejection.
describe('GraphStore variables mutators — the command surface the panel + agent drive', () => {
	beforeEach(() => history().reset());
	const found = (fc: FakeControl, op: string) => fc.recordedCalls().find((c) => c.op === op)?.payload;

	it('addVariable issues variable entry add {name,value} and records an undoable step', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		await g.addVariable('patch.gain', 2.5);
		expect(found(fc, 'variable entry add')).toEqual({ name: 'patch.gain', value: 2.5 });
		expect(history().canUndo).toBe(true);
	});

	it('addVariable refuses a name already taken — set_variable would overwrite it', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		d.variable('patch.gain', { value: 1, system: false });
		await expect(g.addVariable('patch.gain', 2.5)).rejects.toThrow();
		expect(fc.recordedCalls().some((c) => c.op === 'variable entry add')).toBe(false);
	});

	it('setVariableValue issues variable edit — the name and value, nothing else', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		seedSystemVariable(d, 'system.default_ufreq', 30);
		await g.setVariableValue('system.default_ufreq', 45);
		expect(found(fc, 'variable entry edit')).toEqual({ name: 'system.default_ufreq', value: 45 });
	});

	it('setVariableValue rejects an unknown variable (no command sent)', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		await expect(g.setVariableValue('ghost', 1)).rejects.toThrow();
		expect(fc.recordedCalls().some((c) => c.op === 'variable entry edit')).toBe(false);
	});

	it('removeVariable issues variable remove', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		await g.removeVariable('patch.subject');
		expect(found(fc, 'variable entry remove')).toEqual({ name: 'patch.subject' });
	});

	it('renameVariable is ONE op, and the manager rewrites every expression reading it', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		d.variable('patch.gain', { value: 2.5, system: false });
		await g.renameVariable('patch.gain', 'patch.level');
		expect(found(fc, 'variable entry rename')).toEqual({ name: 'patch.gain', to: 'patch.level' });
		expect(history().canUndo).toBe(true);
	});

	it('renameVariableGroup is ONE op too, and moves every member with it', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		d.variable('patch.gain', { value: 2.5, system: false });
		await g.renameVariableGroup('patch', 'desk');
		expect(found(fc, 'variable group rename')).toEqual({ from: 'patch', to: 'desk' });
		expect(history().canUndo).toBe(true);
	});

	it('a server rejection propagates (name/collision/system are validated server-side)', async () => {
		const fc = new FakeControl();
		fc.failNext('variable entry add');
		const g = new GraphStore(fc);
		const d = seed(fc);
		await expect(g.addVariable('bad', 0)).rejects.toThrow();
		// A rejected add records no undo step.
		expect(history().canUndo).toBe(false);
	});
});
