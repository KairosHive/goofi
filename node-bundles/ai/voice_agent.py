"""VoiceAgent — a live call with a speech-to-speech model that can command the patch's agents.

Microphone frames on `audio` (`[C, T]` with `sfreq`, as `signal:AudioIn` streams them) go to the
model, its voice comes back on `audio` at the model's rate, in step with the frames that arrive, so
`audio:SignalIn` plays it; every finished turn's words leave on `transcript`. `model` is
pydantic-ai's `provider:model`: `openai:gpt-realtime`, `google:gemini-live-2.5-flash`,
`xai:grok-voice-latest`; the key is the provider's own environment variable. A text frame on
`text` is a typed turn.

The model has one tool: `command_agent(text)`, which leaves `text` on `command` — link that to an
`Agent` node's `prompt` and the voice steers the coding agent. The call connects while `live` is
on, and reconnects when the model, the voice or the instructions change.
"""

import asyncio
import collections
import threading

import os

import numpy as np
import goofi
os.environ.setdefault("PYDANTIC_AI_NO_BANNER", "1")  # the banner would land in the process log as warnings
from pydantic_ai import Agent
from pydantic_ai.realtime import RealtimeResponseInterruptedEvent

INSTRUCTIONS = (
    "You are the voice of a goofi patch: a live signal-processing graph a person is working on. "
    "Keep replies short. When asked to change the patch, do it with the command_agent tool, "
    "which hands your text to a coding agent that edits the patch; say what you asked for."
)


class VoiceAgent(goofi.Node):
    """A live call with a speech-to-speech model that can command the patch's agents."""

    TAGS = ["control", "text"]
    INPUTS = {
        "audio": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "text": goofi.InputSlot(goofi.DataType.STRING, trigger=False),
    }
    OUTPUTS = {
        "audio": goofi.DataType.ARRAY,
        "transcript": goofi.DataType.STRING,
        "command": goofi.DataType.STRING,
    }
    PARAMS = {
        "voice": {
            "model": goofi.StringParam("openai:gpt-realtime", doc="`provider:model` of a speech-to-speech model."),
            "voice": goofi.StringParam("", doc="The provider's voice name; empty takes its default."),
            "instructions": goofi.StringParam(INSTRUCTIONS, doc="What the model is told at the start of the call."),
            "live": goofi.BoolParam(True, doc="Whether the call is connected."),
        }
    }

    def setup(self):
        self.call = None

    def process(self, audio, text):
        p = self.params.voice
        wanted = (p.model, p.voice, p.instructions) if p.live else None
        if self.call is not None and self.call.key != wanted:
            self.call.hang_up()
            self.call = None
        if wanted is not None and self.call is None:
            self.call = Call(wanted)
        self.clear_input("audio")
        if text is not None:
            self.clear_input("text")
        if self.call is None:
            return None
        if self.call.failure is not None:
            failure, self.call = self.call.failure, None
            raise failure
        sfreq = audio.meta.get("sfreq")
        if not sfreq:
            raise ValueError("audio needs `sfreq` in its meta")
        frame = np.asarray(audio.data, dtype=np.float32)
        mono = frame.reshape(-1, frame.shape[-1]).mean(axis=0) if frame.ndim > 1 else frame
        if text is not None:
            self.call.say(text.text)
        out = self.call.exchange(mono, sfreq)
        emitted = {"audio": (out["audio"][None, :], {"sfreq": float(out["rate"])})}
        if out["transcript"] is not None:
            emitted["transcript"] = out["transcript"]
        if out["command"] is not None:
            emitted["command"] = out["command"]
        return emitted

    def stop(self):
        if self.call is not None:
            self.call.hang_up()


class Call:
    """One connected session on a thread of its own: the mic goes in as it arrives, the voice is
    taken out in step with it, the model's tool calls and words are queued for the next tick."""

    def __init__(self, key):
        self.key = key
        self.failure = None
        self.lock = threading.Lock()
        self.spoken = bytearray()
        self.transcripts = collections.deque()
        self.commands = collections.deque()
        self.rates = None
        self.loop = None
        self.mic = None
        self.texts = None
        self.ready = threading.Event()
        self.thread = threading.Thread(target=self.run, name="goofi-voice-call", daemon=True)
        self.thread.start()
        self.ready.wait()

    def run(self):
        try:
            asyncio.run(self.session())
        except Exception as e:  # noqa: BLE001 — the next tick raises it as the node's error
            self.failure = e
        finally:
            self.ready.set()

    async def session(self):
        model, voice, instructions = self.key
        self.loop = asyncio.get_running_loop()
        self.mic = asyncio.Queue()
        self.texts = asyncio.Queue()
        agent = Agent(instructions=instructions)

        @agent.tool_plain
        def command_agent(text: str) -> str:
            """Hand an instruction to the coding agent that edits the patch."""
            with self.lock:
                self.commands.append(text)
            return "sent to the agent"

        provider = model.split(":", 1)[0]
        settings = {f"{provider}_voice": voice} if voice else None
        async with agent.realtime(model, model_settings=settings).session(handle_barge_in=True) as session:
            self.rates = (session.audio_input_sample_rate, session.audio_output_sample_rate)
            self.ready.set()
            tasks = [
                asyncio.create_task(session.send_audio(self.chunks())),
                asyncio.create_task(self.play(session.stream_audio())),
                asyncio.create_task(self.listen(session)),
                asyncio.create_task(self.type(session)),
            ]
            async for event in session:
                if isinstance(event, RealtimeResponseInterruptedEvent):
                    with self.lock:
                        self.spoken.clear()
            for task in tasks:
                task.cancel()

    async def chunks(self):
        while True:
            chunk = await self.mic.get()
            if chunk is None:
                return
            yield chunk

    async def play(self, audio):
        async for chunk in audio:
            with self.lock:
                self.spoken.extend(chunk)

    async def listen(self, session):
        async for part in session.stream_transcripts():
            with self.lock:
                self.transcripts.append(f"{part.speaker}: {part.transcript}")

    async def type(self, session):
        while True:
            await session.send(await self.texts.get())

    def say(self, text):
        self.loop.call_soon_threadsafe(self.texts.put_nowait, text)

    def exchange(self, mono, sfreq):
        """Send one mic frame; take the voice that plays over the same span, silence where none."""
        rate_in, rate_out = self.rates
        pcm = (np.clip(resample(mono, sfreq, rate_in), -1, 1) * 32767).astype("<i2").tobytes()
        self.loop.call_soon_threadsafe(self.mic.put_nowait, pcm)
        n = int(round(len(mono) / sfreq * rate_out))
        with self.lock:
            take = bytes(self.spoken[: n * 2])
            del self.spoken[: n * 2]
            transcript = self.transcripts.popleft() if self.transcripts else None
            command = self.commands.popleft() if self.commands else None
        audio = np.zeros(n, dtype=np.float32)
        heard = np.frombuffer(take, dtype="<i2").astype(np.float32) / 32768.0
        audio[: len(heard)] = heard
        return {"audio": audio, "rate": rate_out, "transcript": transcript, "command": command}

    def hang_up(self):
        if self.loop is not None and self.loop.is_running():
            self.loop.call_soon_threadsafe(self.mic.put_nowait, None)
            self.loop.call_soon_threadsafe(self.loop.stop)
        self.thread.join(timeout=5)


def resample(mono, sfreq, rate):
    if sfreq == rate:
        return mono.astype(np.float32)
    n = int(round(len(mono) * rate / sfreq))
    return np.interp(np.arange(n) * (sfreq / rate), np.arange(len(mono)), mono).astype(np.float32)
