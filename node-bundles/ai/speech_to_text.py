"""SpeechToText — audio transcribed in segments, by OpenAI, ElevenLabs or an OpenAI-compatible server.

Audio frames (`[C, T]` with `sfreq`, as `signal:AudioIn` streams them) are gathered until
`segment` seconds are held, which go out as one request and come back as one text frame; a
segment quieter than `gate` is silence and is not sent. The key is `OPENAI_API_KEY` or
`ELEVENLABS_API_KEY`; `base_url` points the OpenAI provider at a local server that speaks its API
(Speaches, faster-whisper-server, LocalAI).
"""

import io
import os
import wave

import numpy as np
from elevenlabs.client import ElevenLabs
from openai import OpenAI
import goofi

RATE = 16000


class SpeechToText(goofi.Node):
    """Audio transcribed in segments, by OpenAI, ElevenLabs or an OpenAI-compatible server."""

    TAGS = ["text", "analysis"]
    INPUTS = {"audio": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"text": goofi.DataType.STRING}
    PARAMS = {
        "transcription": {
            "provider": goofi.StringParam("openai", options=["openai", "elevenlabs"]),
            "model": goofi.StringParam("", doc="The provider's transcription model; empty takes its default (gpt-4o-mini-transcribe, or scribe_v1)."),
            "base_url": goofi.StringParam("", doc="An OpenAI-compatible server instead of OpenAI."),
            "segment": goofi.NumParam(5.0, 0.5, 60.0, doc="Seconds of audio sent as one request."),
            "gate": goofi.NumParam(0.01, 0.0, 1.0, doc="A segment whose RMS is below this is silence and is not sent."),
        }
    }

    def setup(self):
        self.client = None
        self.built_from = None
        self.held = []
        self.held_samples = 0

    def process(self, audio):
        p = self.params.transcription
        self.clear_input("audio")
        sfreq = audio.meta.get("sfreq")
        if not sfreq:
            raise ValueError("audio needs `sfreq` in its meta")
        frame = np.asarray(audio.data, dtype=np.float32)
        mono = frame.reshape(-1, frame.shape[-1]).mean(axis=0) if frame.ndim > 1 else frame
        self.held.append(resample(mono, sfreq, RATE))
        self.held_samples += len(self.held[-1])
        if self.held_samples < p.segment * RATE:
            return None
        segment = np.concatenate(self.held)
        self.held, self.held_samples = [], 0
        if np.sqrt(np.mean(segment * segment)) < p.gate:
            return None
        key = (p.provider, p.base_url)
        if key != self.built_from:
            self.client = connect(p.provider, p.base_url)
            self.built_from = key
        return {"text": transcribe(self.client, p.provider, p.model, encode_wav(segment))}


def resample(mono, sfreq, rate):
    if sfreq == rate:
        return mono
    n = int(round(len(mono) * rate / sfreq))
    return np.interp(np.arange(n) * (sfreq / rate), np.arange(len(mono)), mono).astype(np.float32)


def encode_wav(mono):
    out = io.BytesIO()
    with wave.open(out, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes((np.clip(mono, -1, 1) * 32767).astype("<i2").tobytes())
    return out.getvalue()


def connect(provider, base_url):
    if provider == "elevenlabs":
        return ElevenLabs()
    return OpenAI(base_url=base_url or None, api_key=os.environ.get("OPENAI_API_KEY", "none") if base_url else None)


def transcribe(client, provider, model, wav):
    if provider == "elevenlabs":
        return client.speech_to_text.convert(model_id=model or "scribe_v1", file=("segment.wav", wav)).text
    return client.audio.transcriptions.create(model=model or "gpt-4o-mini-transcribe", file=("segment.wav", wav)).text
