import { grayLut, lutTexture, type Program } from './gl';
import { rgba, type Rect } from './math';

/** The most centres a field may sum: the shader's uniform array. */
export const MAX_CENTRES = 128;

export interface FieldSettings {
	/** 256 RGB triplets the window is mapped through. */
	lut: Uint8Array;
	/** Posterize the window into this many bands, so the boundaries read as iso-lines; 0 is smooth. */
	bands: number;
}

/** A thin-plate spline: `a0 + a1 x + a2 y + Σ wᵢ · ½ r² log r²` over the centres, in the field's
 * own unit square, drawn inside a disc of that square. The host solves the weights. */
export interface FieldData {
	/** (x, y, weight) per centre, at most `MAX_CENTRES`. */
	points: Float32Array;
	affine: [number, number, number];
	/** Where the field's unit square sits in the rect, as fractions of it. */
	frame: Rect;
	/** The disc drawn, in field space. */
	disc: { x: number; y: number; r: number };
	/** The window the LUT spans. */
	lo: number;
	hi: number;
}

interface FieldGpu {
	gl: WebGL2RenderingContext;
	prog: Program;
	lutTex: WebGLTexture;
	lutUploaded: Uint8Array | null;
}

export class FieldPlot {
	rect: Rect = { x: 0, y: 0, w: 0, h: 0 };
	order = 0;
	background: [number, number, number, number] = [0, 0, 0, 1];
	private settings: FieldSettings = { lut: grayLut(), bands: 0 };
	private data: FieldData | null = null;
	private gpu: FieldGpu | null = null;

	constructor(private readonly invalidate: () => void, private readonly detach: (p: FieldPlot) => void) {}

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
	setSettings(s: Partial<FieldSettings>): void {
		this.settings = { ...this.settings, ...s };
		this.invalidate();
	}
	remove(): void {
		this.detach(this);
		this.release();
	}
	/** Drop the field: `draw` paints the background alone until the next push. */
	clear(): void {
		this.data = null;
		this.invalidate();
	}
	push(data: FieldData): void {
		this.data = data;
		this.invalidate();
	}

	/** @internal */
	draw(gl: WebGL2RenderingContext, prog: Program, d: Rect, canvas: [number, number]): void {
		const data = this.data;
		if (!data) return;
		const gpu = this.attach(gl, prog);
		gl.useProgram(prog.prog);
		gl.activeTexture(gl.TEXTURE0);
		gl.bindTexture(gl.TEXTURE_2D, gpu.lutTex);
		if (gpu.lutUploaded !== this.settings.lut) {
			gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB8, 256, 1, 0, gl.RGB, gl.UNSIGNED_BYTE, this.settings.lut);
			gpu.lutUploaded = this.settings.lut;
		}
		const u = prog.u;
		const n = Math.min(MAX_CENTRES, Math.floor(data.points.length / 3));
		gl.uniform4f(u.u_rect, d.x, d.y, d.w, d.h);
		gl.uniform2f(u.u_canvas, canvas[0], canvas[1]);
		gl.uniform4f(u.u_frame, data.frame.x, data.frame.y, data.frame.w, data.frame.h);
		gl.uniform3f(u.u_disc, data.disc.x, data.disc.y, data.disc.r);
		gl.uniform3fv(u.u_pts, data.points.subarray(0, n * 3));
		gl.uniform1i(u.u_n, n);
		gl.uniform3f(u.u_affine, ...data.affine);
		gl.uniform1i(u.u_lut, 0);
		gl.uniform1f(u.u_lo, data.lo);
		// A window that is flat or inverted saturates instead of flipping the map.
		gl.uniform1f(u.u_span, data.hi > data.lo ? data.hi - data.lo : 1e-9);
		gl.uniform1f(u.u_bands, this.settings.bands);
		gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
	}

	private attach(gl: WebGL2RenderingContext, prog: Program): FieldGpu {
		if (this.gpu && this.gpu.gl === gl && this.gpu.prog === prog) return this.gpu;
		this.gpu = { gl, prog, lutTex: lutTexture(gl), lutUploaded: null };
		return this.gpu;
	}

	/** @internal */
	release(): void {
		const g = this.gpu;
		if (g && !g.gl.isContextLost()) g.gl.deleteTexture(g.lutTex);
		this.gpu = null;
	}
}

