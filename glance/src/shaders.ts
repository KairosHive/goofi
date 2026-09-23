/** GLSL ES 3.00 sources. Positions arrive in data units and leave in device pixels. */

/** One instance per segment (or per point when `u_point`): `a_p0`/`a_p1` read one buffer a vertex
 * apart. A NaN or out-of-domain endpoint collapses the quad: the row pad, or a log-axis drop. */
export const LINE_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec2 a_p0;
layout(location = 1) in vec2 a_p1;
uniform vec4 u_rect;
uniform vec2 u_canvas;
uniform vec2 u_x;
uniform vec2 u_y;
uniform bvec2 u_log;
uniform float u_width;
uniform int u_stride;
uniform int u_point;
uniform vec4 u_colors[8];
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
void main() {
	v_color = u_colors[(gl_InstanceID / u_stride) % 8];
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
		vec2 d = b - a;
		float len = max(length(d), 1e-6);
		vec2 along = d / len * u_width * 0.5;
		vec2 n = vec2(-d.y, d.x) / len * u_width * 0.5;
		pos = (c < 2 ? a - along : b + along) + ((c & 1) == 1 ? n : -n);
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
