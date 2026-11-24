"""Agent — a coding agent in this patch's workspace, steered by the text it receives.

Every text frame on `prompt` is typed into the agent's terminal and submitted, the way a person
steers it mid-task. `start` launches a new instance of `harness` with `model` and `effort` as its
flags where the harness takes them (claude and codex take both, opencode a model), and this node
steers what it started; `instance` picks a running one instead, started here, in an agent panel or
from the command line. `stop` ends the instance this node steers. Every instance, whoever started
it, shows in the agent panel and in `goofi agent list`.
"""

import goofi


class Agent(goofi.Node):
    """A coding agent in the patch workspace, steered by the text it receives."""

    TAGS = ["control", "text"]
    INPUTS = {"prompt": goofi.InputSlot(goofi.DataType.STRING, required=True)}
    OUTPUTS = {"instance": goofi.DataType.STRING}
    PARAMS = {
        "agent": {
            "harness": goofi.StringParam("claude", refresh=True, doc="Which config-listed agent `start` launches."),
            "model": goofi.StringParam("", doc="The model `start` hands the harness; empty leaves its default."),
            "effort": goofi.StringParam("", doc="The effort `start` hands the harness; empty leaves its default."),
            "instance": goofi.StringParam("", refresh=True, doc="A running instance to steer; empty steers the one this node started."),
            "start": goofi.PulseParam(doc="Launch a new instance of `harness` and steer it."),
            "stop": goofi.PulseParam(doc="Stop the instance this node steers."),
        }
    }

    def setup(self):
        self.owned = None

    def refresh_agent_harness(self):
        return [a["name"] for a in goofi.agent.agents()]

    def refresh_agent_instance(self):
        return [""] + [r["id"] for r in goofi.agent.roster() if r["state"] != "exited"]

    def steered(self):
        """The instance a prompt goes to: the picked one, else the one this node started."""
        return self.params.agent.instance or self.owned

    def pulse_agent_start(self):
        p = self.params.agent
        self.owned = goofi.agent.spawn(p.harness, p.model or None, p.effort or None)

    def pulse_agent_stop(self):
        target = self.steered()
        if target is None:
            raise RuntimeError("no agent to stop: start one or pick an instance")
        goofi.agent.stop(target)
        if target == self.owned:
            self.owned = None

    def process(self, prompt):
        target = self.steered()
        if target is None:
            raise RuntimeError("no agent to steer: start one or pick an instance")
        goofi.agent.send(target, prompt.text)
        self.clear_input("prompt")
        return {"instance": target}

    def stop(self):
        if self.owned is not None:
            goofi.agent.stop(self.owned)
