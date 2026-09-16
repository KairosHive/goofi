"""Virtual audio cables: PipeWire null sinks that carry goofi's audio to and from other software.

A cable is one `pw-cli -m create-node` child. PipeWire owns a created node for as long as the
client that made it lives, so the cable's life is the child's: `remove` kills it, and so does
`on_stop`. The service dying takes every child with it (PDEATHSIG), so a crash leaves nothing.
Windows and macOS have no public API that creates a virtual device; there the panel says so.
"""
from __future__ import annotations

import asyncio
import ctypes
import os
import re
import shutil
import signal
import sys
from dataclasses import dataclass, field
from pathlib import Path

from goofi_plugin import plugin

# The host label cpal gives PipeWire devices; goofi stores a device as `<host>: <description>`.
HOST = "PipeWire"
NODE_PREFIX = "goofi-cable-"
POSITIONS = {1: "MONO", 2: "FL FR", 4: "FL FR RL RR", 6: "FL FR FC LFE RL RR", 8: "FL FR FC LFE RL RR SL SR"}
MAX_CHANNELS = 64

cables: dict[str, "Cable"] = {}


@dataclass
class Cable:
    name: str
    channels: int
    node_name: str
    process: asyncio.subprocess.Process
    stderr: list[str] = field(default_factory=list)

    def describe(self) -> dict:
        return {"name": self.name, "channels": self.channels, "device": f"{HOST}: {self.name}"}


@dataclass
class Empty:
    pass


@dataclass
class Create:
    name: str
    channels: int = 2


@dataclass
class Remove:
    name: str


@dataclass
class Route:
    node: str
    cable: str


def unsupported() -> str | None:
    """Why this machine cannot make a cable, or None when it can."""
    if sys.platform != "linux":
        return "virtual cables need PipeWire; Windows and macOS need a driver such as VB-CABLE or BlackHole"
    if shutil.which("pw-cli") is None:
        return "pw-cli is not installed (package pipewire-utils or pipewire-bin)"
    runtime = os.environ.get("PIPEWIRE_RUNTIME_DIR") or os.environ.get("XDG_RUNTIME_DIR")
    core = os.environ.get("PIPEWIRE_REMOTE", "pipewire-0")
    if not runtime or not (Path(runtime) / core).exists():
        return "PipeWire is not running for this user"
    return None


def positions(channels: int) -> str:
    return POSITIONS.get(channels) or " ".join(f"AUX{i}" for i in range(channels))


def quoted(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def node_name(name: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-") or "cable"
    return f"{NODE_PREFIX}{slug}"


def die_with_parent() -> None:
    libc = ctypes.CDLL("libc.so.6", use_errno=True)
    libc.prctl(1, signal.SIGTERM)  # PR_SET_PDEATHSIG


async def listed(node: str) -> bool:
    """Whether PipeWire lists `node` right now."""
    proc = await asyncio.create_subprocess_exec(
        "pw-cli", "ls", "Node", stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL
    )
    out, _ = await proc.communicate()
    return f'node.name = "{node}"' in out.decode(errors="replace")


async def drain(cable: Cable) -> None:
    assert cable.process.stderr is not None
    async for line in cable.process.stderr:
        cable.stderr.append(line.decode(errors="replace").rstrip())


async def spawn(name: str, channels: int) -> Cable:
    node = node_name(name)
    props = " ".join([
        "factory.name=support.null-audio-sink",
        f"node.name={quoted(node)}",
        f"node.description={quoted(name)}",
        "media.class=Audio/Sink",
        "node.virtual=true",
        f"audio.channels={channels}",
        f"audio.position=[{positions(channels)}]",
        "monitor.channel-volumes=true",
    ])
    process = await asyncio.create_subprocess_exec(
        "pw-cli", "-m", "create-node", "adapter", "{ " + props + " }",
        stdin=asyncio.subprocess.DEVNULL, stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.PIPE,
        preexec_fn=die_with_parent,
    )
    cable = Cable(name, channels, node, process)
    asyncio.create_task(drain(cable))
    for _ in range(40):
        if process.returncode is not None:
            break
        if await listed(node):
            return cable
        await asyncio.sleep(0.05)
    await close(cable)
    detail = "; ".join(cable.stderr) or "PipeWire did not list the node"
    raise RuntimeError(f"could not create cable `{name}`: {detail}")


async def close(cable: Cable) -> None:
    if cable.process.returncode is None:
        cable.process.terminate()
        try:
            await asyncio.wait_for(cable.process.wait(), 2)
        except asyncio.TimeoutError:
            cable.process.kill()
            await cable.process.wait()


def prune() -> None:
    """Forget a cable whose child died under us, so the list tells the truth."""
    for name in [n for n, c in cables.items() if c.process.returncode is not None]:
        del cables[name]


@plugin.on_stop
async def stop(ctx):
    for cable in list(cables.values()):
        await close(cable)
    cables.clear()


@plugin.op("list", kind="read")
async def list_cables(ctx, args: Empty) -> dict:
    """The cables this session holds, and whether this machine can make one."""
    prune()
    return {"unsupported": unsupported(), "cables": [c.describe() for c in cables.values()]}


@plugin.op("create", kind="effect")
async def create(ctx, args: Create) -> dict:
    """Make a virtual cable: an audio device other software sees as an output and an input."""
    reason = unsupported()
    if reason:
        raise RuntimeError(reason)
    name = args.name.strip()
    if not name:
        raise ValueError("a cable needs a name")
    if ": " in name:
        raise ValueError("a cable name cannot contain `: `")
    if not 1 <= args.channels <= MAX_CHANNELS:
        raise ValueError(f"channels must be between 1 and {MAX_CHANNELS}")
    prune()
    if name in cables or any(c.node_name == node_name(name) for c in cables.values()):
        raise ValueError(f"a cable named `{name}` exists")
    cable = await spawn(name, args.channels)
    cables[name] = cable
    ctx.log(f"Cable `{name}` created with {args.channels} channels")
    return cable.describe()


@plugin.op("remove", kind="effect")
async def remove(ctx, args: Remove) -> dict:
    """Remove a cable. A node that named its device keeps the name and reports the device gone."""
    cable = cables.pop(args.name, None)
    if cable is None:
        return {"removed": False}
    await close(cable)
    ctx.log(f"Cable `{args.name}` removed")
    return {"removed": True}


@plugin.op("route", kind="effect")
async def route(ctx, args: Route) -> dict:
    """Point a node at a cable: an AudioIn reads it; an AudioOut sends goofi's output there.
    Every AudioOut names the engine's one clock device, so an AudioOut drop moves all of them."""
    prune()
    cable = cables.get(args.cable)
    if cable is None:
        raise ValueError(f"no cable named `{args.cable}`")
    nodes = (await ctx.call("session state"))["nodes"]
    dropped = nodes.get(args.node)
    if dropped is None:
        raise ValueError(f"no node `{args.node}`")
    kind = dropped.get("type")
    if kind == "audio:AudioOut":
        targets = [uid for uid, node in nodes.items() if node.get("type") == kind]
    elif kind == "audio:AudioIn":
        targets = [args.node]
    else:
        raise ValueError(f"`{dropped.get('name', args.node)}` is {kind}, not an AudioIn or AudioOut")
    device = cable.describe()["device"]
    steps = [{"op": "node param edit", "payload": {"node": uid, "param": "audio/device", "value": device}} for uid in targets]
    await ctx.call("compound", {"ops": steps})
    return {"device": device, "nodes": targets, "direction": "in" if kind == "audio:AudioIn" else "out"}
