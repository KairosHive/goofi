"""TextToSpeech — every text frame spoken, by OpenAI, ElevenLabs or an OpenAI-compatible server.

The speech leaves as one `[1, T]` frame at 24 kHz with `sfreq`, which `audio:SignalIn` plays as
it is. The key is `OPENAI_API_KEY` or `ELEVENLABS_API_KEY`; `base_url` points the OpenAI
provider at a local server that speaks its API (Kokoro-FastAPI, Speaches, LocalAI).
"""

import os

import numpy as np
from elevenlabs.client import ElevenLabs
from openai import OpenAI
import goofi

RATE = 24000


class TextToSpeech(goofi.Node):
    """Every text frame spoken, by OpenAI, ElevenLabs or an OpenAI-compatible server."""

    TAGS = ["text", "generator"]
    INPUTS = {"text": goofi.InputSlot(goofi.DataType.STRING, required=True)}
    OUTPUTS = {"audio": goofi.DataType.ARRAY}
    PARAMS = {
        "speech": {
            "provider": goofi.StringParam("openai", options=["openai", "elevenlabs"]),
            "model": goofi.StringParam("", doc="The provider's speech model; empty takes its default (gpt-4o-mini-tts, or ElevenLabs' own)."),
            "voice": goofi.StringParam("alloy", doc="An OpenAI voice name, or an ElevenLabs voice id."),
            "base_url": goofi.StringParam("", doc="An OpenAI-compatible server instead of OpenAI."),
        }
    }

    def setup(self):
        self.client = None
        self.built_from = None

    def process(self, text):
        p = self.params.speech
        self.clear_input("text")
        key = (p.provider, p.base_url)
        if key != self.built_from:
            self.client = connect(p.provider, p.base_url)
            self.built_from = key
        pcm = speak(self.client, p.provider, p.model, p.voice, text.text)
        samples = np.frombuffer(pcm, dtype="<i2").astype(np.float32) / 32768.0
        return {"audio": (samples[None, :], {"sfreq": float(RATE)})}


def connect(provider, base_url):
    if provider == "elevenlabs":
        return ElevenLabs()
    return OpenAI(base_url=base_url or None, api_key=os.environ.get("OPENAI_API_KEY", "none") if base_url else None)


def speak(client, provider, model, voice, text):
    """Signed 16-bit mono PCM at 24 kHz."""
    if provider == "elevenlabs":
        chunks = client.text_to_speech.convert(voice, text=text, output_format=f"pcm_{RATE}", model_id=model or "eleven_multilingual_v2")
        return b"".join(chunks)
    return client.audio.speech.create(model=model or "gpt-4o-mini-tts", voice=voice, input=text, response_format="pcm").content
