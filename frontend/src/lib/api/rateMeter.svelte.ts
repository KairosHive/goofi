/** A windowed rate counter: `add(n)` counts, `tick(now)` runs on a timer, and `rate` is the
 * latest window's count per second. */
export class RateMeter {
	rate = $state(0);

	private count = 0;
	private windowStart: number;

	constructor(now = performance.now()) {
		this.windowStart = now;
	}

	add(n = 1): void {
		this.count += n;
	}

	tick(now = performance.now()): void {
		const dt = (now - this.windowStart) / 1000;
		if (dt < 0.5) return;
		this.rate = this.count / dt;
		this.count = 0;
		this.windowStart = now;
	}
}
