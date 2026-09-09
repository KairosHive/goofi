"""A session fixture for the public plugin interface."""
from __future__ import annotations
from dataclasses import dataclass
import json
from goofi_plugin import plugin

subject = "none"
mode = "normal"
completed = []

@dataclass
class Empty:
    pass

@dataclass
class Select:
    subject: str

@dataclass
class Mode:
    mode: str

@plugin.on_start
async def start(ctx):
    global subject
    path = ctx.data_dir / "subject.json"
    if path.exists():
        subject = json.loads(path.read_text())
    ctx.log("Session plugin ready")

@plugin.on_stop
async def stop(ctx):
    (ctx.data_dir / "stopped").write_text("yes")

@plugin.op("subject select", kind="effect")
async def select(ctx, args: Select) -> dict:
    """Select the subject for the next recording."""
    global subject
    if not args.subject.isalnum():
        raise ValueError("subject must contain letters or digits")
    subject = args.subject
    (ctx.data_dir / "subject.json").write_text(json.dumps(subject))
    return {"subject": subject}

@plugin.op("status", kind="read")
async def status(ctx, args: Empty) -> dict:
    return {"subject": subject, "completed": completed}

@plugin.op("mode", kind="effect")
async def set_mode(ctx, args: Mode) -> dict:
    global mode
    mode = args.mode
    return {"mode": mode}

@plugin.op("host", kind="read")
async def host(ctx, args: Empty) -> dict:
    return await ctx.call("record status")

@plugin.op("cycle", kind="effect")
async def cycle(ctx, args: Empty) -> dict:
    return await ctx.call("plugin example cycle")

@plugin.pre_op("record start")
async def prepare(ctx, call):
    if mode == "reject":
        call.reject("select a session first")
    if mode == "invalid":
        return call.patch(root=42)
    return call.patch(
        root=str(ctx.data_dir / "recordings" / subject),
        name=f"example-{subject}",
        annotations={"example": {"subject": subject, "tags": ["fixture"]}},
    )

@plugin.post_op("record stop")
async def complete(ctx, outcome):
    if outcome.ok:
        completed.append(outcome.result["folder"])
    if mode == "post-fail":
        raise RuntimeError("upload queue unavailable")

@plugin.pre_op("node add")
async def observe_node(ctx, call):
    return None

@plugin.op("crash", kind="effect")
async def crash(ctx, args: Empty) -> dict:
    import os
    os._exit(9)

@plugin.pre_op("plugin example subject select")
async def normalize_subject(ctx, call):
    return call.patch(subject=call.args["subject"].strip())
