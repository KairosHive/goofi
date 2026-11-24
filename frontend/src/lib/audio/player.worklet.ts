/* The browser device's player, on the audio thread. Decoded frames come in over the port, two
 * channels interleaved, and go out a render quantum at a time from a ring that keeps a little
 * ahead, so a packet that comes late is not heard. */

// What an AudioWorkletGlobalScope holds, which the DOM library does not declare.
declare const sampleRate: number;
declare class AudioWorkletProcessor {
	readonly port: MessagePort;
}
declare function registerProcessor(name: string, processor: new () => AudioWorkletProcessor): void;

const CHANNELS = 2;
/** What the ring holds before it plays, in seconds: what a late packet may take. */
const AHEAD = 0.1;
/** The most it keeps: past that the browser's clock is slower than the engine's, and the oldest go. */
const MOST = 0.5;

class Player extends AudioWorkletProcessor {
	private ring = new Float32Array(Math.ceil(sampleRate * MOST) * CHANNELS);
	private read = 0;
	private held = 0;
	private playing = false;

	constructor() {
		super();
		this.port.onmessage = (e: MessageEvent<Float32Array>) => this.take(e.data);
	}

	/** Frames in. A ring that would overflow is cut back to what it plays ahead. */
	private take(frames: Float32Array): void {
		const size = this.ring.length;
		if (this.held + frames.length > size) {
			const keep = Math.min(this.held, Math.floor(sampleRate * AHEAD) * CHANNELS);
			this.read = (this.read + this.held - keep) % size;
			this.held = keep;
		}
		const from = Math.max(0, frames.length - (size - this.held));
		let write = (this.read + this.held) % size;
		for (let i = from; i < frames.length; i++) {
			this.ring[write] = frames[i];
			write = write + 1 === size ? 0 : write + 1;
		}
		this.held += frames.length - from;
	}

	process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
		const out = outputs[0];
		const frames = out[0].length;
		// A ring that ran dry fills again before it plays: one gap, not a stutter.
		if (this.held < frames * CHANNELS) this.playing = false;
		else if (this.held >= sampleRate * AHEAD * CHANNELS) this.playing = true;
		if (!this.playing) return true;
		const size = this.ring.length;
		for (let i = 0; i < frames; i++) {
			for (let c = 0; c < CHANNELS; c++) {
				if (c < out.length) out[c][i] = this.ring[this.read];
				this.read = this.read + 1 === size ? 0 : this.read + 1;
			}
		}
		this.held -= frames * CHANNELS;
		return true;
	}
}

registerProcessor('goofi-player', Player);
