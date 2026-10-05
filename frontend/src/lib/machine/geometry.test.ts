import { describe, expect, it } from 'vitest';
import { course, flightCourse, type Box, type RouteGeometry } from './geometry';

const boxes: Record<string, Box> = {
	A: { x: 0, y: 0, w: 100, h: 100 },
	B: { x: 400, y: 0, w: 100, h: 100 },
	C: { x: 400, y: 400, w: 100, h: 100 }
};
const box = (id: string): Box | null => boxes[id] ?? null;

describe('a flight keeps its runtime endpoints during authored edits', () => {
	it('shares the connected route when its endpoints still match', () => {
		const connected = course(boxes.A, boxes.B, 60);
		const routes = new Map<string, RouteGeometry>([['t1', {
			key: 't1', id: 't1', source: 'A', target: 'B', course: connected, label: connected.at(0.5)
		}]]);
		expect(flightCourse(routes, 't1', 'A', 'B', box)).toBe(connected);
	});

	it('still reaches the runtime destination after the transition is removed', () => {
		const route = flightCourse(new Map(), 't1', 'A', 'B', box)!;
		expect(route.at(0.5)).toMatchObject({ x: 250, y: 50 });
		expect(route.at(1)).toMatchObject({ x: 400, y: 50 });
	});

	it('does not follow a replacement destination', () => {
		const changed = course(boxes.A, boxes.C);
		const routes = new Map<string, RouteGeometry>([['t1', {
			key: 't1', id: 't1', source: 'A', target: 'C', course: changed, label: changed.at(0.5)
		}]]);
		expect(flightCourse(routes, 't1', 'A', 'B', box)!.at(1)).toMatchObject({ x: 400, y: 50 });
	});

	it('retains a loop after its self-transition is removed', () => {
		const route = flightCourse(new Map(), 't1', 'A', 'A', box)!;
		expect(route.at(0)).toMatchObject({ x: 100, y: 30 });
		expect(route.at(0.5).x).toBeGreaterThan(100);
		expect(route.at(1)).toMatchObject({ x: 100, y: 70 });
	});
});
