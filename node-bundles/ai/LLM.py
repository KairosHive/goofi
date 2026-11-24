"""LLM — one reply per prompt from a language model, any provider, hosted or local.

`model` is pydantic-ai's `provider:model`: `openai:gpt-6-luna`, `anthropic:claude-sonnet-4-5`,
`google:gemini-2.5-flash`, `groq:…`, `mistral:…`, `openrouter:…`, `ollama:llama3.2` for a local
Ollama, `vllm:…`. The key is the provider's own environment variable (`OPENAI_API_KEY`,
`ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, …). `base_url` points the provider at another server:
`openai-chat:<name>` reaches any OpenAI-compatible one, such as LM Studio, llama.cpp or vLLM.
`effort` is pydantic-ai's unified `thinking` setting; pydantic-ai maps it to each provider's own.

Every text frame on `prompt` is one request after `system_prompt`, with the latest `data` frame
appended as its numbers. `single` answers each prompt on its own; `chat` keeps the last `history`
prompts and replies until `clear_history`, the oldest dropped first and the system prompt kept.
With `require_change`, a prompt whose text and `data` equal the last answered request's is skipped.
"""

import dataclasses
import os
from typing import get_args

import numpy as np
os.environ.setdefault("PYDANTIC_AI_NO_BANNER", "1")  # the banner would land in the process log as warnings
from pydantic_ai import Agent
from pydantic_ai.messages import ModelRequest, SystemPromptPart, UserPromptPart
from pydantic_ai.models import infer_model
from pydantic_ai.providers import infer_provider_class
from pydantic_ai.settings import ThinkingEffort
import goofi

# The dropdown's words for pydantic-ai's thinking levels that are not effort names.
THINKING = {"default": None, "off": False, "on": True}


class LLM(goofi.Node):
    """One reply per prompt from a language model, any provider, hosted or local."""

    TAGS = ["text", "ml"]
    INPUTS = {
        "prompt": goofi.InputSlot(goofi.DataType.STRING, required=True),
        "data": goofi.InputSlot(goofi.DataType.ARRAY, trigger=False),
    }
    OUTPUTS = {"out": goofi.DataType.STRING}
    PARAMS = {
        "model": {
            "model": goofi.StringParam("openai:gpt-6-luna", doc="`provider:model`, the key from the provider's environment variable."),
            "base_url": goofi.StringParam("", doc="A server to send the request to instead of the provider's own."),
            "effort": goofi.StringParam("default", options=[*THINKING, *get_args(ThinkingEffort)], doc="How hard the model thinks; `default` leaves it to the model."),
            "system_prompt": goofi.StringParam("", doc="What the model is told before the conversation."),
            "mode": goofi.StringParam("chat", options=["chat", "single"], doc="`chat` keeps the conversation; `single` answers each prompt alone."),
            "history": goofi.NumParam(20, 1, 100, int=True, show=("mode", ["chat"]), doc="How many earlier prompts and replies the model sees."),
            "clear_history": goofi.PulseParam(doc="Forget the conversation.", show=("mode", ["chat"])),
            "require_change": goofi.BoolParam(True, doc="Skip a prompt whose text and data equal the last answered request's."),
        }
    }

    def setup(self):
        self.agent = None
        self.built_from = None
        self.history = []
        self.answered = None

    def pulse_model_clear_history(self):
        self.history = []

    def process(self, prompt, data):
        p = self.params.model
        key = (p.model, p.base_url, p.system_prompt)
        if key != self.built_from:
            self.agent = Agent(resolve(p.model, p.base_url), system_prompt=p.system_prompt or ())
            self.built_from = key
            self.history = []
        self.clear_input("prompt")
        request = (prompt.text, None if data is None else np.array(data.data))
        if p.require_change and self.answered is not None and same(self.answered, request):
            return None
        text = prompt.text
        if data is not None:
            values = np.array2string(data.data, precision=4, separator=", ", threshold=4096)
            text = f"{text}\n\ndata, shape {list(data.data.shape)}:\n{values}"
        chat = p.mode == "chat"
        thinking = THINKING.get(p.effort, p.effort)
        result = self.agent.run_sync(
            text,
            message_history=keep_last(self.history, int(p.history)) if chat else None,
            model_settings=None if thinking is None else {"thinking": thinking},
        )
        if chat:
            self.history = result.all_messages()
        self.answered = request
        return {"out": result.output}


def same(a, b):
    """Whether two requests have the same text and the same data, shape included."""
    (text_a, data_a), (text_b, data_b) = a, b
    if data_a is None or data_b is None:
        return text_a == text_b and data_a is data_b
    return text_a == text_b and np.array_equal(data_a, data_b)


def keep_last(messages, turns):
    """The last `turns` prompts with their replies, the system prompt moved to the first kept prompt."""
    starts = [i for i, m in enumerate(messages) if isinstance(m, ModelRequest) and any(isinstance(part, UserPromptPart) for part in m.parts)]
    if len(starts) <= turns:
        return messages
    system = [part for part in messages[0].parts if isinstance(part, SystemPromptPart)]
    kept = messages[starts[-turns] :]
    kept[0] = dataclasses.replace(kept[0], parts=[*system, *kept[0].parts])
    return kept


def resolve(model, base_url):
    """pydantic-ai's model string, its provider pointed at `base_url` when one is given."""
    if not base_url:
        return model
    return infer_model(model, provider_factory=lambda name: infer_provider_class(name)(base_url=base_url))
