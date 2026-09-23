import { bindSegments, type Program } from './gl';
import {
	extent,
	layoutSeries,
	nearestIndex,
	rgba,
	unmapped,
	axisWindow,
	gridLines,
	type Rect
} from './math';

export interface LineSettings {
	logX: boolean;
	logY: boolean;
	yAuto: boolean;
	yMin: number;
	yMax: number;
	points: boolean;
}

export interface LineData {
	/** One row per series, all of one length. A single row of length 1 is a scalar. */
	rows: ArrayLike<number>[];
	/** Sample positions shared by every row; the index from `base` when absent. */
	xs?: ArrayLike<number> | null;
	base?: number;
}

export interface Range {
	xMin: number;
	xMax: number;
	yMin: number;
	yMax: number;
	/** A scalar bar: only the x range carries meaning. */
	scalar: boolean;
}

const GRID_COLOR = new Float32Array(32).map((_, i) => (i % 4 === 3 ? 0.05 : 1));

/** Grid segments in rect fractions: a NaN pad after each line keeps the instanced draw from joining them. */
function gridSegments(xs: number[], ys: number[]): Float32Array {
	return new Float32Array([
		...xs.flatMap((t) => [t, 0, t, 1, NaN, NaN]),
		...ys.flatMap((t) => [0, t, 1, t, NaN, NaN])
	]);
}

/** The GL handles a line plot owns; rebuilt after a context loss from the CPU copy. */
interface LineGpu {
	gl: WebGL2RenderingContext;
	prog: Program;
	vbo: WebGLBuffer;
	grid: WebGLBuffer;
	uploaded: Float32Array | null;
	gridUploaded: Float32Array | null;
}

export class LinePlot {
	rect: Rect = { x: 0, y: 0, w: 0, h: 0 };
	order = 0;
	background: [number, number, number, number] = [0, 0, 0, 1];
	private colors = new Float32Array(32);
	private settings: LineSettings = { logX: false, logY: false, yAuto: true, yMin: -1, yMax: 1, points: false };
	private buf: Float32Array | null = null;
	private stride = 0;
	private m = 0;
	private series = 0;
	private scalar = false;
	private scalarLo = Infinity;
	private scalarHi = -Infinity;
	private xw: [number, number] = [0, 1];
	private yw: [number, number] = [0, 1];
	private grid = gridSegments([], []);
	private gpu: LineGpu | null = null;

	constructor(private readonly invalidate: () => void, private readonly detach: (p: LinePlot) => void) {
		this.setColors(['#ffffff']);
	}

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
	/** Up to eight `#rrggbb` series colours; a ninth series wraps to the first. */
	setColors(hex: string[]): void {
		for (let i = 0; i < 8; i++) this.colors.set(rgba(hex[i % hex.length]), i * 4);
		this.invalidate();
	}
	setSettings(s: Partial<LineSettings>): void {
		this.settings = { ...this.settings, ...s };
		this.fitWindows();
		this.invalidate();
	}
	remove(): void {
		this.detach(this);
		this.release();
	}
	/** Drop the series: `draw` paints the background alone and `range()` is null until the next push. */
	clear(): void {
		this.buf = null;
		this.m = 0;
		this.scalar = false;
		if (this.gpu) this.gpu.uploaded = null;
		this.invalidate();
	}

	push(data: LineData): void {
		const { rows } = data;
		const scalar = rows.length === 1 && rows[0].length === 1;
		if (scalar) {
			const v = Number(rows[0][0]);
			if (!this.scalar) {
				this.scalarLo = Infinity;
				this.scalarHi = -Infinity;
			}
			if (Number.isFinite(v)) {
				this.scalarLo = Math.min(this.scalarLo, v);
				this.scalarHi = Math.max(this.scalarHi, v);
			}
			// A bar at x = value, standing from y 0 to 1.
			const laid = layoutSeries([[0, 1]], [v, v], 0, this.buf);
			this.buf = laid.buf;
			this.stride = laid.stride;
			this.m = laid.m;
		} else {
			const laid = layoutSeries(rows, data.xs ?? null, data.base ?? 0, this.buf);
			this.buf = laid.buf;
			this.stride = laid.stride;
			this.m = laid.m;
		}
		this.scalar = scalar;
		this.series = scalar ? 1 : rows.length;
		this.fitWindows();
		if (this.gpu) this.gpu.uploaded = null;
		this.invalidate();
	}

	range(): Range | null {
		if (!this.buf || this.m === 0) return null;
		const { logX, logY } = this.settings;
		if (this.scalar) {
			return { xMin: this.xw[0], xMax: this.xw[1], yMin: 0, yMax: 1, scalar: true };
		}
		return {
			xMin: unmapped(this.xw[0], logX),
			xMax: unmapped(this.xw[1], logX),
			yMin: unmapped(this.yw[0], logY),
			yMax: unmapped(this.yw[1], logY),
			scalar: false
		};
	}

	/** The sample nearest a point `x` flow units from the rect's left edge, one value per series. */
	valueAt(x: number): { x: number; values: number[] } | null {
		const buf = this.buf;
		if (!buf || this.scalar || this.m === 0 || this.rect.w <= 0) return null;
		const t = Math.min(1, Math.max(0, x / this.rect.w));
		const target = unmapped(this.xw[0] + t * (this.xw[1] - this.xw[0]), this.settings.logX);
		const i = nearestIndex((k) => buf[k * 2], this.m, target);
		if (i < 0) return null;
		const values: number[] = [];
		for (let s = 0; s < this.series; s++) values.push(buf[(s * this.stride + i) * 2 + 1]);
		return { x: buf[i * 2], values };
	}

	private fitWindows(): void {
		const buf = this.buf;
		if (!buf || this.m === 0) return;
		const { logX, logY, yAuto, yMin, yMax } = this.settings;
		if (this.scalar) {
			this.fitScalar(buf, yAuto, yMin, yMax);
		} else {
			this.xw = axisWindow(buf[0], buf[(this.m - 1) * 2], logX, 0);
			if (yAuto) {
				const e = extent(buf, 1, 2, logY) ?? [-1, 1];
				this.yw = axisWindow(e[0], e[1], logY);
			} else {
				this.yw = axisWindow(yMin, yMax, logY, 0);
			}
		}
		const log = this.scalar ? [false, false] : [logX, logY];
		this.grid = gridSegments(
			gridLines(this.xw[0], this.xw[1], log[0]),
			gridLines(this.yw[0], this.yw[1], log[1], 3)
		);
	}

	private fitScalar(buf: Float32Array, yAuto: boolean, yMin: number, yMax: number): void {
		this.yw = [0, 1];
		if (!yAuto && Number.isFinite(yMin) && Number.isFinite(yMax) && yMin !== yMax) {
			this.xw = yMin < yMax ? [yMin, yMax] : [yMax, yMin];
		} else if (this.scalarLo <= this.scalarHi) {
			this.xw = axisWindow(this.scalarLo, this.scalarHi, false);
		} else {
			this.xw = [buf[0] - 1, buf[0] + 1];
		}
	}

	/** @internal Draw into `d`, the rect on the device; the surface has set scissor and cleared. */
	draw(gl: WebGL2RenderingContext, prog: Program, d: Rect, canvas: [number, number], dpr: number): void {
		if (!this.buf || this.m === 0) return;
		const gpu = this.attach(gl, prog);
		if (gpu.uploaded !== this.buf) {
			gl.bindBuffer(gl.ARRAY_BUFFER, gpu.vbo);
			gl.bufferData(gl.ARRAY_BUFFER, this.buf, gl.DYNAMIC_DRAW);
			gpu.uploaded = this.buf;
		}
		const u = prog.u;
		gl.useProgram(prog.prog);
		// A small margin so a line on the range edge is not cut by the rect.
		const pad = Math.round(2 * dpr);
		gl.uniform4f(u.u_rect, d.x + pad, d.y + pad, d.w - 2 * pad, d.h - 2 * pad);
		gl.uniform2f(u.u_canvas, canvas[0], canvas[1]);

		bindSegments(gl, gpu.grid);
		if (gpu.gridUploaded !== this.grid) {
			gl.bufferData(gl.ARRAY_BUFFER, this.grid, gl.DYNAMIC_DRAW);
			gpu.gridUploaded = this.grid;
		}
		gl.uniform2f(u.u_x, 0, 1);
		gl.uniform2f(u.u_y, 0, 1);
		gl.uniform2i(u.u_log, 0, 0);
		gl.uniform1f(u.u_width, Math.max(1, Math.round(dpr)));
		gl.uniform1i(u.u_stride, this.grid.length);
		gl.uniform1i(u.u_point, 0);
		gl.uniform4fv(u.u_colors, GRID_COLOR);
		if (this.grid.length > 2) gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, this.grid.length / 2 - 1);

		bindSegments(gl, gpu.vbo);
		gl.uniform2f(u.u_x, this.xw[0], this.xw[1]);
		gl.uniform2f(u.u_y, this.yw[0], this.yw[1]);
		gl.uniform2i(u.u_log, this.scalar ? 0 : +this.settings.logX, this.scalar ? 0 : +this.settings.logY);
		gl.uniform1f(u.u_width, Math.max(1, dpr) * (this.scalar ? 2 : 1));
		gl.uniform1i(u.u_stride, this.stride);
		gl.uniform4fv(u.u_colors, this.colors);
		const instances = this.series * this.stride - 1;
		gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, instances);
		if (this.settings.points && !this.scalar) {
			gl.uniform1i(u.u_point, 1);
			gl.uniform1f(u.u_width, 2 * Math.max(1, dpr));
			gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, instances);
		}
	}

	private attach(gl: WebGL2RenderingContext, prog: Program): LineGpu {
		if (this.gpu && this.gpu.gl === gl && this.gpu.prog === prog) return this.gpu;
		this.gpu = { gl, prog, vbo: gl.createBuffer()!, grid: gl.createBuffer()!, uploaded: null, gridUploaded: null };
		return this.gpu;
	}

	/** @internal Drop the GL handles; the CPU copy stays for the next attach. */
	release(): void {
		const g = this.gpu;
		if (g && !g.gl.isContextLost()) {
			g.gl.deleteBuffer(g.vbo);
			g.gl.deleteBuffer(g.grid);
		}
		this.gpu = null;
	}
}
