import { describe, it, expect } from 'vitest';
import { NODE, nodeSurfaceSize, inputPorts, inputUnits } from './nodeMetrics';

describe('nodeSurfaceSize', () => {
	it('a node with one collapsed output slot is header + one unit tall, node-width wide', () => {
		const s = nodeSurfaceSize(0, [false]);
		expect(s.width).toBe(NODE.width);
		expect(s.height).toBe(NODE.header + NODE.unit);
	});

	it('an expanded output slot adds the viewer plot height', () => {
		const s = nodeSurfaceSize(0, [true]);
		expect(s.height).toBe(NODE.header + NODE.unit + NODE.viewer);
	});

	it('a slotless node is still at least header + one unit (the input connector pitch)', () => {
		const s = nodeSurfaceSize(0, []);
		expect(s.height).toBe(NODE.header + NODE.unit);
	});

	it('grows to fit many input connectors when inputs out-tall the output-slot stack', () => {
		const s = nodeSurfaceSize(5, []); // 5 input units, no outputs
		expect(s.height).toBe(NODE.header + 5 * NODE.unit);
	});

	it('uses the taller of the input body and the output-slot stack', () => {
		// 1 input (1 unit) vs 2 collapsed outputs (2 units) → outputs win.
		const s = nodeSurfaceSize(1, [false, false]);
		expect(s.height).toBe(NODE.header + 2 * NODE.unit);
	});

});

describe('inputUnits', () => {
	it('every slot counts one unit', () => {
		expect(inputUnits(['a', 'b', 'c'])).toBe(3);
	});
	it('floors at one so a slotless node still has a body', () => {
		expect(inputUnits([])).toBe(1);
	});
});

describe('inputPorts (stacked placement)', () => {
	const base = NODE.border + NODE.header;
	it('stacks single slots one unit apart, centred in each unit', () => {
		const ports = inputPorts(['a', 'b']);
		expect(ports[0].top).toBe(base + NODE.unit / 2);
		expect(ports[1].top).toBe(base + NODE.unit + NODE.unit / 2);
	});
	it('preserves slot order', () => {
		expect(inputPorts(['x', 'y', 'z']).map((p) => p.slot)).toEqual(['x', 'y', 'z']);
	});
});
