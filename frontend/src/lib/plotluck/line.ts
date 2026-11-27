import { bindSegments, type Program } from './gl';
import { BasePlot } from './plot';
import { seriesRgba } from './color';
import {
	extent,
	layoutSeries,
	perRow,
	settled,
	unmapped,
	axisWindow,
	gridLines,
	type Rect,
	type Xs
} from './math';

export interface LineSettings {
	logX: boolean;
	logY: boolean;
	yAuto: boolean;
	yMin: number;
	yMax: number;
	points: boolean;
	/** The stroke in CSS px. */
	width: number;
	/** A point's side in CSS px; 0 draws it four strokes wide. */
	pointWidth: number;
	/** The series' opacity, 0..1; the grid keeps its own. */
	alpha: number;
	/** One window for both axes: the extent of the x and y values together, or the manual y range,
	 * so a phase portrait keeps its shape; `logX` and `logY` do not apply. */
	square: boolean;
}

export interface LineData {
	/** One row per series, all of one length. A single row of length 1 is a scalar. */
	rows: ArrayLike<number>[];
	/** Sample positions: one row shared by every series, or one row per series (a trajectory);
	 * the index from `base` when absent. A shared row must not decrease. */
	xs?: Xs | null;
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

const GRID_COLOR: [number, number, number, number] = [1, 1, 1, 0.05];
const MINOR_COLOR: [number, number, number, number] = [1, 1, 1, 0.02];

/** Grid segments in rect fractions: a NaN pad after each line keeps the instanced draw from joining them. */
function gridSegments(xs: number[], ys: number[]): Float32Array {
	return new Float32Array([
		NaN,
		NaN,
		...xs.flatMap((t) => [t, 0, t, 1, NaN, NaN]),
		...ys.flatMap((t) => [0, t, 1, t, NaN, NaN]),
		NaN,
		NaN
	]);
}

/** The GL handles a line plot owns; rebuilt after a context loss from the CPU copy. */
interface LineGpu {
	gl: WebGL2RenderingContext;
	prog: Program;
	vbo: WebGLBuffer;
	grid: WebGLBuffer;
	minor: WebGLBuffer;
	palette: WebGLTexture;
	/** How many series the palette texture holds. */
	paletteSeries: number;
	uploaded: Float32Array | null;
	gridUploaded: Float32Array | null;
	minorUploaded: Float32Array | null;
}

export class LinePlot extends BasePlot {
	private settings: LineSettings = {
		logX: false,
		logY: false,
		yAuto: true,
		yMin: -1,
		yMax: 1,
		points: false,
		width: 1,
		pointWidth: 0,
		alpha: 1,
		square: false
	};
	private buf: Float32Array | null = null;
	private stride = 0;
	private m = 0;
	private series = 0;
	private scalar = false;
	/** Whether each series has an x row of its own, so the x window is read from every x. */
	private rowXs = false;
	private scalarLo = Infinity;
	private scalarHi = -Infinity;
	private xw: [number, number] = [0, 1];
	private yw: [number, number] = [0, 1];
	/** The autoscaled window the last fit showed; null when the next fit starts afresh. */
	private auto: [number, number] | null = null;
	private grid = gridSegments([], []);
	private minor = gridSegments([], []);
	private gpu: LineGpu | null = null;

	setSettings(s: Partial<LineSettings>): void {
		this.settings = { ...this.settings, ...s };
		this.auto = null;
		this.fitWindows();
		this.invalidate();
	}
	/** Drop the series: `draw` paints the background alone and `range()` is null until the next push. */
	clear(): void {
		this.buf = null;
		this.m = 0;
		this.scalar = false;
		this.auto = null;
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
		this.rowXs = !scalar && perRow(data.xs);
		this.scalar = scalar;
		this.series = scalar ? 1 : rows.length;
		this.fitWindows();
		if (this.gpu) this.gpu.uploaded = null;
		this.invalidate();
	}

	range(): Range | null {
		if (!this.buf || this.m === 0) return null;
		const logX = this.settings.logX && !this.settings.square;
		const logY = this.settings.logY && !this.settings.square;
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

	private fitWindows(): void {
		const buf = this.buf;
		if (!buf || this.m === 0) return;
		const { yAuto, yMin, yMax, square } = this.settings;
		const logX = this.settings.logX && !square;
		const logY = this.settings.logY && !square;
		if (this.scalar) {
			this.fitScalar(buf, yAuto, yMin, yMax);
		} else if (square) {
			const e = yAuto ? (extent(buf) ?? [-1, 1]) : [yMin, yMax];
			this.xw = axisWindow(e[0], e[1], false, yAuto ? 0.05 : 0);
			if (yAuto) this.xw = this.auto = settled(this.auto, this.xw);
			this.yw = this.xw;
		} else {
			// Point k sits at buf[(k + 1) * 2]: the pad comes first.
			const x = this.rowXs ? (extent(buf, 0, 2, logX) ?? [0, 1]) : [buf[2], buf[this.m * 2]];
			this.xw = axisWindow(x[0], x[1], logX, 0);
			if (yAuto) {
				const e = extent(buf, 1, 2, logY) ?? [-1, 1];
				this.yw = this.auto = settled(this.auto, axisWindow(e[0], e[1], logY));
			} else {
				this.yw = axisWindow(yMin, yMax, logY, 0);
			}
		}
		const log = this.scalar ? [false, false] : [logX, logY];
		const gx = gridLines(this.xw[0], this.xw[1], log[0]);
		const gy = gridLines(this.yw[0], this.yw[1], log[1], 3);
		this.grid = gridSegments(gx.major, gy.major);
		this.minor = gridSegments(gx.minor, gy.minor);
	}

	private fitScalar(buf: Float32Array, yAuto: boolean, yMin: number, yMax: number): void {
		this.yw = [0, 1];
		if (!yAuto && Number.isFinite(yMin) && Number.isFinite(yMax) && yMin !== yMax) {
			this.xw = yMin < yMax ? [yMin, yMax] : [yMax, yMin];
		} else if (this.scalarLo <= this.scalarHi) {
			this.xw = axisWindow(this.scalarLo, this.scalarHi, false);
		} else {
			this.xw = [buf[2] - 1, buf[2] + 1];
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

		gl.uniform2f(u.u_x, 0, 1);
		gl.uniform2f(u.u_y, 0, 1);
		gl.uniform2i(u.u_log, 0, 0);
		gl.uniform1f(u.u_width, Math.max(1, Math.round(dpr)));
		gl.uniform1i(u.u_point, 0);
		gl.uniform1i(u.u_palette, 0);
		this.drawGrid(gl, u, gpu.minor, this.minor, gpu.minorUploaded, MINOR_COLOR);
		gpu.minorUploaded = this.minor;
		this.drawGrid(gl, u, gpu.grid, this.grid, gpu.gridUploaded, GRID_COLOR);
		gpu.gridUploaded = this.grid;

		bindSegments(gl, gpu.vbo);
		gl.uniform2f(u.u_x, this.xw[0], this.xw[1]);
		gl.uniform2f(u.u_y, this.yw[0], this.yw[1]);
		const plain = this.scalar || this.settings.square;
		gl.uniform2i(u.u_log, plain ? 0 : +this.settings.logX, plain ? 0 : +this.settings.logY);
		const stroke = Math.max(1, dpr) * this.settings.width;
		gl.uniform1f(u.u_width, stroke * (this.scalar ? 2 : 1));
		gl.uniform1f(u.u_alpha, this.settings.alpha);
		gl.uniform1i(u.u_stride, this.stride);
		gl.uniform4f(u.u_flat, 0, 0, 0, 0);
		gl.activeTexture(gl.TEXTURE0);
		gl.bindTexture(gl.TEXTURE_2D, gpu.palette);
		if (gpu.paletteSeries < this.series) {
			const n = Math.max(this.series, gpu.paletteSeries * 2, 8);
			const px = new Uint8Array(n * 4);
			for (let i = 0; i < n; i++) px.set(seriesRgba(i).map((v) => Math.round(v * 255)), i * 4);
			gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, n, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, px);
			gpu.paletteSeries = n;
		}
		// Instance i is the segment from point i + 1: the pads before and after keep every read in range.
		const instances = this.series * this.stride - 2;
		gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, instances);
		if (this.settings.points && !this.scalar) {
			gl.uniform1i(u.u_point, 1);
			// The shader takes the half-size of the point's square.
			const half = this.settings.pointWidth > 0 ? (Math.max(1, dpr) * this.settings.pointWidth) / 2 : 2 * stroke;
			gl.uniform1f(u.u_width, half);
			gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, instances + 1);
		}
	}

	private drawGrid(
		gl: WebGL2RenderingContext,
		u: Program['u'],
		vbo: WebGLBuffer,
		lines: Float32Array,
		uploaded: Float32Array | null,
		color: [number, number, number, number]
	): void {
		if (lines.length <= 4) return;
		bindSegments(gl, vbo);
		if (uploaded !== lines) gl.bufferData(gl.ARRAY_BUFFER, lines, gl.DYNAMIC_DRAW);
		gl.uniform1i(u.u_stride, lines.length);
		gl.uniform4f(u.u_flat, ...color);
		gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, lines.length / 2 - 3);
	}

	private attach(gl: WebGL2RenderingContext, prog: Program): LineGpu {
		if (this.gpu && this.gpu.gl === gl && this.gpu.prog === prog) return this.gpu;
		const palette = gl.createTexture()!;
		gl.bindTexture(gl.TEXTURE_2D, palette);
		gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
		gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
		this.gpu = {
			gl,
			prog,
			vbo: gl.createBuffer()!,
			grid: gl.createBuffer()!,
			minor: gl.createBuffer()!,
			palette,
			paletteSeries: 0,
			uploaded: null,
			gridUploaded: null,
			minorUploaded: null
		};
		return this.gpu;
	}

	/** @internal Drop the GL handles; the CPU copy stays for the next attach. */
	release(): void {
		const g = this.gpu;
		if (g && !g.gl.isContextLost()) {
			g.gl.deleteBuffer(g.vbo);
			g.gl.deleteBuffer(g.grid);
			g.gl.deleteBuffer(g.minor);
			g.gl.deleteTexture(g.palette);
		}
		this.gpu = null;
	}
}
