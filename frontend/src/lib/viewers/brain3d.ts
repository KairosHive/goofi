/** The camera the 3-D brain is seen through, and the projection of a point on the head. */

export interface Camera {
	/** Turn about the vertical, in radians; 0 looks along the anterior axis. */
	yaw: number;
	/** Tilt from top-down, in radians: 0 looks straight down at the vertex, π/2 from the front. */
	pitch: number;
}

export interface Projected {
	x: number;
	y: number;
	/** 0 at the far side of the head, 1 at the near, for the dot's size and weight. */
	depth: number;
}

/** A projection of head coordinates (x right, y anterior, z up) onto a `w`×`h` box, with a
 * little perspective so the near side reads as near. */
export function project(camera: Camera, w: number, h: number): (p: [number, number, number]) => Projected {
	const cy = Math.cos(camera.yaw);
	const sy = Math.sin(camera.yaw);
	const cp = Math.cos(camera.pitch);
	const sp = Math.sin(camera.pitch);
	const scale = Math.min(w, h) * 0.38;
	const distance = 4;
	return ([x, y, z]) => {
		// Yaw about z, then pitch about x: the vertex comes toward the viewer at pitch 0.
		const x1 = x * cy - y * sy;
		const y1 = x * sy + y * cy;
		const y2 = y1 * cp - z * sp;
		const z2 = y1 * sp + z * cp;
		const persp = distance / (distance - z2);
		return {
			x: w / 2 + x1 * scale * persp,
			y: h / 2 - y2 * scale * persp,
			depth: (z2 + 1) / 2
		};
	};
}
