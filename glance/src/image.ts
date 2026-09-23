import type { Program } from './gl';
import { fitImage, rgba, type Rect } from './math';

export interface ImageSettings {
	/** 256 RGB triplets the single-channel mode samples through. */
	lut: Uint8Array;
	stretch: boolean;
}

export interface ImageData {
	values: Uint8Array | Float32Array;
	width: number;
	height: number;
	/** 1, 2, 3 or 4 interleaved channels: LUT, red and green, rgb, rgba. */
	channels: number;
	/** The window a windowed channel (modes 1 and 2) is mapped over, in texture sample units. */
	lo: number;
	hi: number;
}

interface ImageGpu {
	gl: WebGL2RenderingContext;
	prog: Program;
	tex: WebGLTexture;
	lutTex: WebGLTexture;
	uploaded: ImageData | null;
	lutUploaded: Uint8Array | null;
	/** The storage `tex` holds, so a frame of the same shape uploads in place. */
	shape: [number, number, number] | null;
	floatLinear: boolean;
}

export class ImagePlot {
	rect: Rect = { x: 0, y: 0, w: 0, h: 0 };
	order = 0;
	background: [number, number, number, number] = [0, 0, 0, 1];
	private settings: ImageSettings = { lut: grayLut(), stretch: false };
	private data: ImageData | null = null;
	private gpu: ImageGpu | null = null;

	constructor(private readonly invalidate: () => void, private readonly detach: (p: ImagePlot) => void) {}

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
	setSettings(s: Partial<ImageSettings>): void {
		this.settings = { ...this.settings, ...s };
		this.invalidate();
	}
	remove(): void {
		this.detach(this);
		this.release();
	}
	/** Drop the image: `draw` paints the background alone until the next push. */
	clear(): void {
		this.data = null;
		if (this.gpu) this.gpu.uploaded = null;
		this.invalidate();
	}

	/** The caller's array is kept, not copied: a frame is never written after it is decoded. */
	push(data: ImageData): void {
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
		gl.bindTexture(gl.TEXTURE_2D, gpu.tex);
		if (gpu.uploaded !== data) {
			upload(gl, gpu, data);
			gpu.uploaded = data;
		}
		gl.activeTexture(gl.TEXTURE1);
		gl.bindTexture(gl.TEXTURE_2D, gpu.lutTex);
		if (gpu.lutUploaded !== this.settings.lut) {
			gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB8, 256, 1, 0, gl.RGB, gl.UNSIGNED_BYTE, this.settings.lut);
			gpu.lutUploaded = this.settings.lut;
		}
		const fit = fitImage(d, data.width, data.height, this.settings.stretch);
		const u = prog.u;
		gl.uniform4f(u.u_rect, fit.x, fit.y, fit.w, fit.h);
		gl.uniform2f(u.u_canvas, canvas[0], canvas[1]);
		gl.uniform1i(u.u_tex, 0);
		gl.uniform1i(u.u_lut, 1);
		gl.uniform1i(u.u_mode, [0, 3, 1, 2][data.channels - 1]);
		gl.uniform1f(u.u_lo, data.lo);
		// A window that is flat or inverted saturates instead of flipping the map.
		gl.uniform1f(u.u_span, data.hi > data.lo ? data.hi - data.lo : 1e-9);
		gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
	}

	private attach(gl: WebGL2RenderingContext, prog: Program): ImageGpu {
		if (this.gpu && this.gpu.gl === gl && this.gpu.prog === prog) return this.gpu;
		const lutTex = gl.createTexture()!;
		gl.bindTexture(gl.TEXTURE_2D, lutTex);
		clampTexture(gl, gl.LINEAR, gl.LINEAR);
		const tex = gl.createTexture()!;
		gl.bindTexture(gl.TEXTURE_2D, tex);
		clampTexture(gl, gl.LINEAR, gl.NEAREST);
		const floatLinear = gl.getExtension('OES_texture_float_linear') !== null;
		this.gpu = { gl, prog, tex, lutTex, uploaded: null, lutUploaded: null, shape: null, floatLinear };
		return this.gpu;
	}

	/** @internal */
	release(): void {
		const g = this.gpu;
		if (g && !g.gl.isContextLost()) {
			g.gl.deleteTexture(g.tex);
			g.gl.deleteTexture(g.lutTex);
		}
		this.gpu = null;
	}
}

function clampTexture(gl: WebGL2RenderingContext, min: number, mag: number): void {
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, min);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, mag);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
}

/** Smooth when shrinking, blocky when enlarging: a texel stays a texel. The storage is
 *  allocated once per shape; a frame of the same shape is written into it. */
function upload(gl: WebGL2RenderingContext, gpu: ImageGpu, d: ImageData): void {
	const u8 = d.values instanceof Uint8Array;
	const c = d.channels;
	const internal = u8
		? [gl.R8, gl.RG8, gl.RGB8, gl.RGBA8][c - 1]
		: [gl.R32F, gl.RG32F, gl.RGB32F, gl.RGBA32F][c - 1];
	const format = [gl.RED, gl.RG, gl.RGB, gl.RGBA][c - 1];
	const type = u8 ? gl.UNSIGNED_BYTE : gl.FLOAT;
	gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
	const s = gpu.shape;
	if (s && s[0] === d.width && s[1] === d.height && s[2] === internal) {
		gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, d.width, d.height, format, type, d.values);
		return;
	}
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, u8 || gpu.floatLinear ? gl.LINEAR : gl.NEAREST);
	gl.texImage2D(gl.TEXTURE_2D, 0, internal, d.width, d.height, 0, format, type, d.values);
	gpu.shape = [d.width, d.height, internal];
}

function grayLut(): Uint8Array {
	const lut = new Uint8Array(768);
	for (let i = 0; i < 256; i++) lut.fill(i, i * 3, i * 3 + 3);
	return lut;
}
