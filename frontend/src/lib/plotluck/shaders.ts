/** GLSL ES 3.00 sources. Positions arrive in data units and leave in device pixels. */

/** One instance per segment (or point); a NaN or out-of-domain endpoint collapses the quad. The
 * quad's ends lie on the miter with each neighbouring segment, so joints share one edge and no
 * pixel is painted twice; a pad or a bad neighbour makes a butt end. The colour is the series'
 * texel of `u_palette`, or `u_flat` when its alpha is set (the grid). */
export const LINE_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec2 a_prev;
layout(location = 1) in vec2 a_p0;
layout(location = 2) in vec2 a_p1;
layout(location = 3) in vec2 a_next;
uniform vec4 u_rect;
uniform vec2 u_canvas;
uniform vec2 u_x;
uniform vec2 u_y;
uniform bvec2 u_log;
uniform float u_width;
uniform int u_stride;
uniform int u_point;
uniform sampler2D u_palette;
uniform vec4 u_flat;
uniform float u_alpha;
out vec4 v_color;

bool ok(vec2 p) {
	if (any(isnan(p)) || any(isinf(p))) return false;
	return (!u_log.x || p.x > 0.0) && (!u_log.y || p.y > 0.0);
}
vec2 toPx(vec2 p) {
	vec2 m = vec2(u_log.x ? log2(p.x) * 0.30103 : p.x, u_log.y ? log2(p.y) * 0.30103 : p.y);
	vec2 t = (m - vec2(u_x.x, u_y.x)) / vec2(u_x.y - u_x.x, u_y.y - u_y.x);
	return vec2(u_rect.x + t.x * u_rect.z, u_rect.y + (1.0 - t.y) * u_rect.w);
}
// The half-width offset at the joint of a segment with normal n and its neighbour's normal n2:
// along the miter, no longer than four half-widths, so a spike does not throw a spear.
vec2 joint(vec2 n, vec2 n2, float hw) {
	vec2 sum = n + n2;
	if (dot(sum, sum) < 1e-6) return n * hw;
	vec2 m = normalize(sum);
	return m * hw / max(dot(m, n), 0.25);
}
// The unit normal of the segment from p to q, or false where it is too short to have one.
bool normalOf(vec2 p, vec2 q, out vec2 n) {
	vec2 d = q - p;
	float len = length(d);
	if (len < 1e-4) return false;
	n = vec2(-d.y, d.x) / len;
	return true;
}
void main() {
	vec4 series = texelFetch(u_palette, ivec2(gl_InstanceID / u_stride, 0), 0);
	v_color = u_flat.a > 0.0 ? u_flat : vec4(series.rgb, series.a * u_alpha);
	bool bad = !ok(a_p0) || (u_point == 0 && !ok(a_p1));
	if (bad) {
		gl_Position = vec4(-2.0, -2.0, 0.0, 1.0);
		return;
	}
	vec2 a = toPx(a_p0);
	int c = gl_VertexID;
	vec2 pos;
	if (u_point == 1) {
		pos = a + vec2((c & 1) == 1 ? u_width : -u_width, c >= 2 ? u_width : -u_width);
	} else {
		vec2 b = toPx(a_p1);
		vec2 n;
		if (!normalOf(a, b, n)) {
			gl_Position = vec4(-2.0, -2.0, 0.0, 1.0);
			return;
		}
		float hw = u_width * 0.5;
		vec2 offA = n * hw;
		vec2 offB = n * hw;
		vec2 n2;
		if (ok(a_prev) && normalOf(toPx(a_prev), a, n2)) offA = joint(n, n2, hw);
		if (ok(a_next) && normalOf(b, toPx(a_next), n2)) offB = joint(n, n2, hw);
		pos = c < 2 ? a + ((c & 1) == 1 ? offA : -offA) : b + ((c & 1) == 1 ? offB : -offB);
	}
	vec2 clip = pos / u_canvas * 2.0 - 1.0;
	gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}`;

export const LINE_FS = `#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 o;
void main() { o = v_color; }`;

/** A quad over `u_rect`; row 0 of the texture lands at the top. */
export const IMAGE_VS = `#version 300 es
precision highp float;
uniform vec4 u_rect;
uniform vec2 u_canvas;
out vec2 v_uv;
void main() {
	int c = gl_VertexID;
	v_uv = vec2((c & 1) == 1 ? 1.0 : 0.0, c >= 2 ? 1.0 : 0.0);
	vec2 pos = u_rect.xy + v_uv * u_rect.zw;
	vec2 clip = pos / u_canvas * 2.0 - 1.0;
	gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}`;

/** `u_mode` 0 = one channel through the LUT, 1 = rgb, 2 = rgba, 3 = two channels as red and green. */
export const IMAGE_FS = `#version 300 es
precision highp float;
in vec2 v_uv;
uniform sampler2D u_tex;
uniform sampler2D u_lut;
uniform int u_mode;
uniform float u_lo;
uniform float u_span;
out vec4 o;
void main() {
	if (u_mode == 0) {
		float t = clamp((texture(u_tex, v_uv).r - u_lo) / u_span, 0.0, 1.0);
		o = vec4(texture(u_lut, vec2(t, 0.5)).rgb, 1.0);
	} else if (u_mode == 1) {
		o = vec4(texture(u_tex, v_uv).rgb, 1.0);
	} else if (u_mode == 2) {
		o = texture(u_tex, v_uv);
	} else {
		o = vec4(clamp((texture(u_tex, v_uv).rg - u_lo) / u_span, 0.0, 1.0), 0.0, 1.0);
	}
}`;

/** One instance per stroke segment (or dot), as `LINE_VS`, with the position in rect fractions and
 * the width and colour carried per vertex; a dot is a quad of its diameter, rounded in the fragment. */
export const PATH_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec2 a_prev;
layout(location = 1) in vec3 a_p0;
layout(location = 2) in vec4 a_c0;
layout(location = 3) in vec3 a_p1;
layout(location = 4) in vec4 a_c1;
layout(location = 5) in vec2 a_next;
uniform vec4 u_rect;
uniform vec2 u_canvas;
uniform float u_scale;
uniform int u_point;
out vec4 v_color;
out vec2 v_corner;

bool ok(vec2 p) {
	return !(any(isnan(p)) || any(isinf(p)));
}
vec2 toPx(vec2 p) {
	return u_rect.xy + p * u_rect.zw;
}
vec2 joint(vec2 n, vec2 n2, float hw) {
	vec2 sum = n + n2;
	if (dot(sum, sum) < 1e-6) return n * hw;
	vec2 m = normalize(sum);
	return m * hw / max(dot(m, n), 0.25);
}
bool normalOf(vec2 p, vec2 q, out vec2 n) {
	vec2 d = q - p;
	float len = length(d);
	if (len < 1e-4) return false;
	n = vec2(-d.y, d.x) / len;
	return true;
}
void main() {
	int c = gl_VertexID;
	v_corner = vec2((c & 1) == 1 ? 1.0 : -1.0, c >= 2 ? 1.0 : -1.0);
	if (!ok(a_p0.xy) || (u_point == 0 && !ok(a_p1.xy))) {
		gl_Position = vec4(-2.0, -2.0, 0.0, 1.0);
		v_color = vec4(0.0);
		return;
	}
	vec2 a = toPx(a_p0.xy);
	vec2 pos;
	if (u_point == 1) {
		v_color = a_c0;
		pos = a + v_corner * a_p0.z * u_scale * 0.5;
	} else {
		v_color = c < 2 ? a_c0 : a_c1;
		vec2 b = toPx(a_p1.xy);
		vec2 n;
		if (!normalOf(a, b, n)) {
			gl_Position = vec4(-2.0, -2.0, 0.0, 1.0);
			return;
		}
		float hwA = a_p0.z * u_scale * 0.5;
		float hwB = a_p1.z * u_scale * 0.5;
		vec2 offA = n * hwA;
		vec2 offB = n * hwB;
		vec2 n2;
		if (ok(a_prev) && normalOf(toPx(a_prev), a, n2)) offA = joint(n, n2, hwA);
		if (ok(a_next) && normalOf(b, toPx(a_next), n2)) offB = joint(n, n2, hwB);
		pos = c < 2 ? a + ((c & 1) == 1 ? offA : -offA) : b + ((c & 1) == 1 ? offB : -offB);
	}
	vec2 clip = pos / u_canvas * 2.0 - 1.0;
	gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}`;

export const PATH_FS = `#version 300 es
precision mediump float;
in vec4 v_color;
in vec2 v_corner;
uniform highp int u_point;
out vec4 o;
void main() {
	float a = v_color.a;
	if (u_point == 1) {
		float d = length(v_corner);
		float aa = fwidth(d);
		if (d > 1.0 + aa) discard;
		a *= 1.0 - smoothstep(1.0 - aa, 1.0 + aa, d);
	}
	o = vec4(v_color.rgb, a);
}`;

/** The thin-plate spline over the centres, read at every fragment of the disc and mapped through
 * the LUT; the disc's rim is anti-aliased and everything outside it is left to the background. */
export const FIELD_FS = `#version 300 es
precision highp float;
in vec2 v_uv;
uniform vec4 u_frame;
uniform vec3 u_disc;
uniform vec3 u_pts[128];
uniform int u_n;
uniform vec3 u_affine;
uniform sampler2D u_lut;
uniform float u_lo;
uniform float u_span;
uniform float u_bands;
out vec4 o;
void main() {
	vec2 q = (v_uv - u_frame.xy) / u_frame.zw;
	float d = length(q - u_disc.xy);
	float aa = fwidth(d);
	if (d > u_disc.z + aa) discard;
	float v = u_affine.x + u_affine.y * q.x + u_affine.z * q.y;
	for (int i = 0; i < 128; i++) {
		if (i >= u_n) break;
		vec2 e = q - u_pts[i].xy;
		float r2 = dot(e, e);
		if (r2 > 1e-12) v += u_pts[i].z * 0.5 * r2 * log(r2);
	}
	float t = clamp((v - u_lo) / u_span, 0.0, 1.0);
	if (u_bands > 0.0) t = floor(t * u_bands + 0.5) / u_bands;
	float rim = 1.0 - smoothstep(u_disc.z - aa, u_disc.z + aa, d);
	o = vec4(texture(u_lut, vec2(t, 0.5)).rgb, rim);
}`;
