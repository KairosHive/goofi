"""Decision — which of the possible answers a text calls for, by a decision model.

A decision model answers a question about a text with probabilities and writes no text of its own.
`model` is pydantic-ai's `provider:model`: `openai-decisions:gpt-6-luna`, `typesafe:jev-latest`, or
`system-one:<model>` for a server on the `/v1/systemone` API, a local Ollama included, which
`base_url` points at. The key is the provider's own environment variable (`OPENAI_API_KEY`,
`TYPESAFE_API_KEY`, `SYSTEM_ONE_API_KEY`).

Every text frame on `text` is judged against `question`, and the model picks one of the answers
on `options`: one per line, each `label` or `label: what it means`. `answer` is the picked
label and `probabilities` holds one per answer, labelled. With `require_change`, a text and
answers equal to the last answered ones are skipped.
"""

import os

import numpy as np
os.environ.setdefault("PYDANTIC_AI_NO_BANNER", "1")  # the banner would land in the process log as warnings
from pydantic_ai import Agent, Choice, Choices
from pydantic_ai.models import infer_model
from pydantic_ai.providers import infer_provider_class
import goofi

# The field pydantic-ai wraps a bare output type in, which keys the answer's details.
FIELD = "response"


class Decision(goofi.Node):
    """Which of the possible answers a text calls for, by a decision model."""

    TAGS = ["text", "ml"]
    INPUTS = {
        "text": goofi.InputSlot(goofi.DataType.STRING, required=True),
        "options": goofi.InputSlot(goofi.DataType.STRING, required=True, trigger=False),
    }
    OUTPUTS = {"answer": goofi.DataType.STRING, "probabilities": goofi.DataType.ARRAY}
    PARAMS = {
        "decision": {
            "model": goofi.StringParam("openai-decisions:gpt-6-luna", doc="`provider:model`, the key from the provider's environment variable."),
            "base_url": goofi.StringParam("", doc="A server to send the request to instead of the provider's own."),
            "question": goofi.StringParam("Which answer fits the text?", doc="What to decide about each text."),
            "require_change": goofi.BoolParam(True, doc="Skip a text and answers equal to the last answered ones."),
        }
    }

    def setup(self):
        self.agent = None
        self.built_from = None
        self.answered = None

    def process(self, text, options):
        p = self.params.decision
        key = (p.model, p.base_url, p.question)
        if key != self.built_from:
            self.agent = Agent(resolve(p.model, p.base_url), instructions=p.question or None)
            self.built_from = key
            self.answered = None
        self.clear_input("text")
        if options is None:
            raise ValueError("`options` takes the possible answers, one per line")
        request = (text.text, options.text)
        # A decision model refuses a request with no text to judge, which an empty Text node sends.
        if not text.text.strip() or (p.require_change and request == self.answered):
            return None
        labels, output_type = answers(options.text)
        result = self.agent.run_sync(text.text, output_type=output_type)
        self.answered = request
        given = (result.response.provider_details or {}).get("probabilities", {}).get(FIELD, {})
        probabilities = np.array([given.get(label, 0.0) for label in labels], dtype=np.float32)
        return {"answer": result.output, "probabilities": (probabilities, {"channels": {"dim0": labels}})}


def answers(options):
    """The answer labels in order and the output type that picks one of them."""
    items = [item.strip() for item in options.splitlines() if item.strip()]
    described = dict(split(item) for item in items)
    if len(described) < 2:
        raise ValueError("`options` takes at least two answers, one per line")
    if len(described) < len(items):
        raise ValueError("`options` names an answer twice")
    return list(described), Choices({label: Choice(meaning or None) for label, meaning in described.items()})


def split(item):
    """An answer's label and what it means, which is empty when the item names only the label."""
    label, _, meaning = item.partition(":")
    return label.strip(), meaning.strip()


def resolve(model, base_url):
    """pydantic-ai's model string, its provider pointed at `base_url` when one is given."""
    if not base_url:
        return model
    return infer_model(model, provider_factory=lambda name: infer_provider_class(name)(base_url=base_url))
