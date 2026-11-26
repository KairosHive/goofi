import { bindPath, type Program } from './gl';
import { rgba, type Rect } from './math';

/** A colour as floats in 0..1. */
export type Rgba = [number, number, number, number];

/** Seven floats per vertex: x and y as rect fractions (y down), the width or diameter in CSS px,
 * then the colour. A NaN vertex ends a stroke. */
export const PATH_STRIDE = 7;

export interface PathData {
	/** The strokes' vertices, padded as `paths()` lays them out. */
	strokes: Float32Array;
	/** One vertex per dot, padded the same way. */
	dots: Float32Array;
}

const PAD = [NaN, NaN, NaN, NaN, NaN, NaN, NaN];

/** Strokes and dots for a `PathPlot`, in the rect's fractions: x right and y down, 0..1. */
export class PathBuilder {
	private strokes: number[] = [...PAD];
	private dots: number[] = [...PAD];

	/** A polyline through `points` (x, y pairs), `width` CSS px wide; a NaN point breaks it. */
	stroke(points: ArrayLike<number>, width: number, color: Rgba | string): this {
		const c = typeof color === 'string' ? rgba(color) : color;
		for (let i = 0; i + 1 < points.length; i += 2) {
			this.strokes.push(points[i], points[i + 1], width, c[0], c[1], c[2], c[3]);
		}
		this.strokes.push(...PAD);
		return this;
	}

	/** A round dot at (`x`, `y`) of `diameter` CSS px. */
	dot(x: number, y: number, diameter: number, color: Rgba | string): this {
		const c = typeof color === 'string' ? rgba(color) : color;
		this.dots.push(x, y, diameter, c[0], c[1], c[2], c[3]);
		return this;
	}

	build(): PathData {
		// One pad before and two after, so every instance reads a neighbour on each side.
		return {
			strokes: Float32Array.from([...this.strokes, ...PAD]),
			dots: Float32Array.from([...this.dots, ...PAD, ...PAD])
		};
	}
}

export function paths(): PathBuilder {
	return new PathBuilder();
}

interface PathGpu {
	gl: WebGL2RenderingContext;
	prog: Program;
	strokes: WebGLBuffer;
	dots: WebGLBuffer;
	uploaded: PathData | null;
}

/** Strokes and dots the host lays out itself, in fractions of the rect: a picture the surface
 * has no data model for, such as a diagram or a projection. */
export class PathPlot {
	rect: Rect = { x: 0, y: 0, w: 0, h: 0 };
	order = 0;
	background: [number, number, number, number] = [0, 0, 0, 1];
	private data: PathData | null = null;
	private gpu: PathGpu | null = null;

	constructor(private readonly invalidate: () => void, private readonly detach: (p: PathPlot) => void) {}

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
		this.detach(this);
		this.release();
	}
	/** Drop the picture: `draw` paints the background alone until the next push. */
	clear(): void {
		this.data = null;
		if (this.gpu) this.gpu.uploaded = null;
		this.invalidate();
	}
	push(data: PathData): void {
		this.data = data;
		this.invalidate();
	}

	/** @internal Draw into `d`, the rect on the device; the surface has set scissor and cleared. */
	draw(gl: WebGL2RenderingContext, prog: Program, d: Rect, canvas: [number, number], dpr: number): void {
		const data = this.data;
		if (!data) return;
		const gpu = this.attach(gl, prog);
		const u = prog.u;
		gl.useProgram(prog.prog);
		gl.uniform4f(u.u_rect, d.x, d.y, d.w, d.h);
		gl.uniform2f(u.u_canvas, canvas[0], canvas[1]);
		gl.uniform1f(u.u_scale, Math.max(1, dpr));
		const fresh = gpu.uploaded !== data;
		const strokes = data.strokes.length / PATH_STRIDE - 3;
		if (strokes > 0) {
			bindPath(gl, gpu.strokes);
			if (fresh) gl.bufferData(gl.ARRAY_BUFFER, data.strokes, gl.DYNAMIC_DRAW);
			gl.uniform1i(u.u_point, 0);
			gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, strokes);
		}
		const dots = data.dots.length / PATH_STRIDE - 3;
		if (dots > 0) {
			bindPath(gl, gpu.dots);
			if (fresh) gl.bufferData(gl.ARRAY_BUFFER, data.dots, gl.DYNAMIC_DRAW);
			gl.uniform1i(u.u_point, 1);
			gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, dots);
		}
		gpu.uploaded = data;
	}

	private attach(gl: WebGL2RenderingContext, prog: Program): PathGpu {
		if (this.gpu && this.gpu.gl === gl && this.gpu.prog === prog) return this.gpu;
		this.gpu = { gl, prog, strokes: gl.createBuffer()!, dots: gl.createBuffer()!, uploaded: null };
		return this.gpu;
	}

	/** @internal Drop the GL handles; the CPU copy stays for the next attach. */
	release(): void {
		const g = this.gpu;
		if (g && !g.gl.isContextLost()) {
			g.gl.deleteBuffer(g.strokes);
			g.gl.deleteBuffer(g.dots);
		}
		this.gpu = null;
	}
}
