import { rgba, type Rect } from './math';

/** What every plot on a surface has: its rect, its place in the stack, its background, and the
 * way it leaves. A change asks the surface for a frame. */
export abstract class BasePlot {
	rect: Rect = { x: 0, y: 0, w: 0, h: 0 };
	order = 0;
	background: [number, number, number, number] = [0, 0, 0, 1];

	constructor(protected readonly invalidate: () => void, private readonly detach: () => void) {}

	setRect(x: number, y: number, w: number, h: number): void {
		this.rect = { x, y, w, h };
		this.invalidate();
	}
	setOrder(z: number): void {
		this.order = z;
		this.invalidate();
	}
	setBackground(hex: string): void {
		this.background = rgba(hex);
		this.invalidate();
	}
	remove(): void {
		this.detach();
		this.release();
	}
	/** @internal Free what the plot holds on the GPU. */
	abstract release(): void;
}
