/** Program construction. A compile error is a bug in plotluck, so it throws. */

export interface Program {
	prog: WebGLProgram;
	u: Record<string, WebGLUniformLocation>;
	/** The attribute state the program draws with; `null` is the default one, which has none. */
	vao: WebGLVertexArrayObject | null;
}

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
	const sh = gl.createShader(type)!;
	gl.shaderSource(sh, src);
	gl.compileShader(sh);
	if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
		throw new Error(`plotluck shader: ${gl.getShaderInfoLog(sh)}`);
	}
	return sh;
}

export function program(gl: WebGL2RenderingContext, vs: string, fs: string): Program {
	const prog = gl.createProgram()!;
	gl.attachShader(prog, compile(gl, gl.VERTEX_SHADER, vs));
	gl.attachShader(prog, compile(gl, gl.FRAGMENT_SHADER, fs));
	gl.linkProgram(prog);
	if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
		throw new Error(`plotluck program: ${gl.getProgramInfoLog(prog)}`);
	}
	const u: Record<string, WebGLUniformLocation> = {};
	const n = gl.getProgramParameter(prog, gl.ACTIVE_UNIFORMS) as number;
	for (let i = 0; i < n; i++) {
		const name = gl.getActiveUniform(prog, i)!.name.replace('[0]', '');
		u[name] = gl.getUniformLocation(prog, name)!;
	}
	return { prog, u, vao: null };
}

/** A segment buffer bound to the line program's four attribute slots, one point apart. */
export function bindSegments(gl: WebGL2RenderingContext, vbo: WebGLBuffer): void {
	gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
	// Slot k reads point k of the window: the previous point, the two ends, the next point.
	for (let k = 0; k < 4; k++) {
		gl.enableVertexAttribArray(k);
		gl.vertexAttribPointer(k, 2, gl.FLOAT, false, 8, 8 * k);
		gl.vertexAttribDivisor(k, 1);
	}
}

/** A path buffer bound to the path program's six attribute slots: the previous point, each end
 * with its width and colour, and the next point, one vertex apart. */
export function bindPath(gl: WebGL2RenderingContext, vbo: WebGLBuffer): void {
	gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
	const stride = 28;
	const layout: [number, number, number][] = [
		[0, 2, 0],
		[1, 3, 28],
		[2, 4, 40],
		[3, 3, 56],
		[4, 4, 68],
		[5, 2, 84]
	];
	for (const [slot, size, offset] of layout) {
		gl.enableVertexAttribArray(slot);
		gl.vertexAttribPointer(slot, size, gl.FLOAT, false, stride, offset);
		gl.vertexAttribDivisor(slot, 1);
	}
}

/** A 256-wide RGB texture for a LUT, sampled linearly and clamped. */
export function lutTexture(gl: WebGL2RenderingContext): WebGLTexture {
	const tex = gl.createTexture()!;
	gl.bindTexture(gl.TEXTURE_2D, tex);
	clampTexture(gl, gl.LINEAR, gl.LINEAR);
	return tex;
}

export function clampTexture(gl: WebGL2RenderingContext, min: number, mag: number): void {
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, min);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, mag);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
}

/** The identity LUT: 256 grey levels. */
export function grayLut(): Uint8Array {
	const lut = new Uint8Array(768);
	for (let i = 0; i < 256; i++) lut.fill(i, i * 3, i * 3 + 3);
	return lut;
}
