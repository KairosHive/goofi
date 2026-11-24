/* Hearing the browser device: the mix the engine sends on `/audio`, decoded here and played by one
 * worklet. It is off until a press, because a browser starts no sound without one. Opus where
 * the browser decodes it, 16-bit PCM where it does not. */
import playerUrl from '$lib/audio/player.worklet.ts?worker&url';
import { wsUrl } from '$lib/api/wsUrl';

/** The browser device's rate and width: the engine's `BROWSER_RATE` and `BROWSER_CHANNELS`. */
const RATE = 48000;
const CHANNELS = 2;
/** A packet's head: the index of its first frame, as a 64-bit integer. */
const HEAD = 8;

const OPUS: AudioDecoderConfig = { codec: 'opus', sampleRate: RATE, numberOfChannels: CHANNELS };

async function decodesOpus(): Promise<boolean> {
	if (typeof AudioDecoder === 'undefined') return false;
	return AudioDecoder.isConfigSupported(OPUS).then((s) => s.supported === true, () => false);
}

export class ListenStore {
	/** Whether this tab plays the browser device. */
	on = $state(false);
	/** Why it could not, where it could not. */
	error = $state<string | null>(null);
	/** The frames given to the player since it was turned on. Raw: nothing draws it. */
	frames = 0;
	private ctx: AudioContext | null = null;
	private ws: WebSocket | null = null;
	private decoder: AudioDecoder | null = null;

	toggle(): void {
		if (this.on) this.stop();
		else void this.start();
	}

	/** Open the context, the player and the socket. Called from a press. */
	async start(): Promise<void> {
		this.stop();
		this.on = true;
		this.error = null;
		this.frames = 0;
		try {
			const ctx = (this.ctx = new AudioContext({ sampleRate: RATE, latencyHint: 'interactive' }));
			await ctx.audioWorklet.addModule(playerUrl);
			const opus = await decodesOpus();
			// Turned off, or on again, while those answered: that press owns the state now.
			if (this.ctx !== ctx) return;
			const player = new AudioWorkletNode(ctx, 'goofi-player', { numberOfInputs: 0, outputChannelCount: [CHANNELS] });
			player.connect(ctx.destination);
			const play = (frames: Float32Array): void => {
				this.frames += frames.length / CHANNELS;
				player.port.postMessage(frames, [frames.buffer]);
			};
			if (opus) this.decoder = this.opusDecoder(play);
			const ws = (this.ws = new WebSocket(`${wsUrl(['audio'])}?codec=${opus ? 'opus' : 'pcm'}`));
			ws.binaryType = 'arraybuffer';
			ws.onmessage = (e: MessageEvent<ArrayBuffer | string>) => {
				// The text message says what the stream is, which this side already asked for.
				if (typeof e.data === 'string' || e.data.byteLength <= HEAD) return;
				if (this.decoder) this.decode(e.data);
				else play(pcm(e.data));
			};
			ws.onclose = () => {
				if (this.ws === ws) this.fail('the audio stream closed');
			};
		} catch (e) {
			this.fail(e instanceof Error ? e.message : String(e));
		}
	}

	stop(): void {
		const [ws, decoder, ctx] = [this.ws, this.decoder, this.ctx];
		this.ws = this.decoder = this.ctx = null;
		this.on = false;
		ws?.close();
		if (decoder && decoder.state !== 'closed') decoder.close();
		void ctx?.close();
	}

	private fail(why: string): void {
		this.stop();
		this.error = why;
	}

	private opusDecoder(play: (frames: Float32Array) => void): AudioDecoder {
		const decoder = new AudioDecoder({
			output: (data) => {
				// Every decoder gives planar floats when asked; the player takes them interleaved.
				const n = data.numberOfFrames;
				const [frames, plane] = [new Float32Array(n * CHANNELS), new Float32Array(n)];
				for (let c = 0; c < CHANNELS; c++) {
					data.copyTo(plane, { planeIndex: Math.min(c, data.numberOfChannels - 1), format: 'f32-planar' });
					for (let i = 0; i < n; i++) frames[i * CHANNELS + c] = plane[i];
				}
				data.close();
				play(frames);
			},
			error: (e) => this.fail(e.message)
		});
		decoder.configure(OPUS);
		return decoder;
	}

	private decode(packet: ArrayBuffer): void {
		const first = Number(new DataView(packet).getBigUint64(0, true));
		this.decoder?.decode(new EncodedAudioChunk({ type: 'key', timestamp: Math.round((first / RATE) * 1e6), data: new Uint8Array(packet, HEAD) }));
	}
}

/** A PCM packet's frames: 16-bit words, little-endian, interleaved. */
function pcm(packet: ArrayBuffer): Float32Array {
	const view = new DataView(packet, HEAD);
	const frames = new Float32Array(view.byteLength / 2);
	for (let i = 0; i < frames.length; i++) frames[i] = view.getInt16(i * 2, true) / 32768;
	return frames;
}

let store: ListenStore | null = null;

export function listen(): ListenStore {
	return (store ??= new ListenStore());
}
