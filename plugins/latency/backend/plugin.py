"""The latency from one node to another, summed hop by hop along the wires between them, one
line per tick of the target.

Every frame carries its `source`: the frames it was made from, each with its node, index, tick
time and the instant it left, and each with its own sources behind it. So a target frame alone
names the chain back to the source node, and the hops along the path between the two sum to that
one tick's latency, whatever else ran in between. The recorder is the one path that keeps every
frame, so a run arms the target's output, records it under the plugin's data folder, and tails
its sidecar as it is written. A tick whose chain does not reach the source is unpaired.
"""
from __future__ import annotations

import asyncio
import json
import math
from collections import deque
from dataclasses import dataclass, field
from pathlib import Path

from goofi_plugin import plugin

TAIL_EVERY = 0.1
FIXED = ["target_time", "target_index", "source_time", "source_index", "latency_ms"]


@dataclass
class Empty:
    pass


@dataclass
class Start:
    source: str
    target: str
    source_slot: str | None = None
    target_slot: str | None = None


@dataclass
class Hop:
    """A node after the source on the path: its uid, and the slot the path enters it on."""
    uid: str
    name: str
    slot_in: str
    total_ms: float = 0.0
    count: int = 0


@dataclass
class Run:
    source: str
    source_name: str
    source_slot: str
    target: str
    target_name: str
    target_slot: str
    hops: list[Hop]
    folder: Path
    csv: Path
    armed: bool = False
    path: Path | None = None
    offset: int = 0
    carried: dict = field(default_factory=dict)
    frames: int = 0
    task: asyncio.Task | None = None
    paired: int = 0
    unpaired: int = 0
    last_ms: float | None = None
    total_ms: float = 0.0
    min_ms: float = math.inf
    max_ms: float = -math.inf
    error: str | None = None
    stopping: bool = False

    def describe(self, running: bool) -> dict:
        return {
            "running": running,
            "source": {"uid": self.source, "name": self.source_name, "slot": self.source_slot},
            "target": {"uid": self.target, "name": self.target_name, "slot": self.target_slot, "frames": self.frames},
            "path": [self.source_name] + [h.name for h in self.hops],
            "hops": [
                {"node": h.name, "slot": h.slot_in, "mean_ms": h.total_ms / h.count if h.count else None}
                for h in self.hops
            ],
            "folder": str(self.folder),
            "csv": str(self.csv),
            "ticks": self.paired,
            "unpaired": self.unpaired,
            "last_ms": self.last_ms,
            "mean_ms": self.total_ms / self.paired if self.paired else None,
            "min_ms": self.min_ms if self.paired else None,
            "max_ms": self.max_ms if self.paired else None,
            "error": self.error,
        }


run: Run | None = None
last: Run | None = None


def uid_of(nodes: dict, ref: str, role: str) -> str:
    if ref in nodes:
        return ref
    uid = next((u for u, n in nodes.items() if n.get("name") == ref), None)
    if uid is None:
        raise ValueError(f"no node `{ref}` for the {role}")
    return uid


def route(links: dict, source: str, target: str, source_slot: str | None) -> list[dict]:
    """The fewest wires from `source` to `target`, the first leaving by `source_slot` if named."""
    parent: dict[str, dict | None] = {source: None}
    queue = deque([source])
    while queue and target not in parent:
        at = queue.popleft()
        for link in links.values():
            if link["node_out"] != at or link["node_in"] in parent:
                continue
            if at == source and source_slot is not None and link["slot_out"] != source_slot:
                continue
            parent[link["node_in"]] = link
            queue.append(link["node_in"])
    if target not in parent:
        raise ValueError("no path along the wires from the source to the target")
    path = []
    at = target
    while parent[at] is not None:
        path.append(parent[at])
        at = parent[at]["node_out"]
    path.reverse()
    return path


async def plan(ctx, args: Start) -> Run:
    state = await ctx.call("session state")
    nodes = state["nodes"]
    source = uid_of(nodes, args.source, "source")
    target = uid_of(nodes, args.target, "target")
    if source == target:
        raise ValueError("the source and the target are the same node")
    links = route(state.get("links", {}), source, target, args.source_slot)
    info = await ctx.call("library get", {"type": nodes[target]["type"]})
    outputs = list(info.get("output_slots", {}).keys())
    if not outputs:
        raise ValueError(f"`{nodes[target]['name']}` has no output to time")
    target_slot = args.target_slot or outputs[0]
    if target_slot not in outputs:
        raise ValueError(f"`{nodes[target]['name']}` has no output `{target_slot}`")
    hops = [Hop(uid=l["node_in"], name=nodes[l["node_in"]]["name"], slot_in=l["slot_in"]) for l in links]
    return Run(
        source=source,
        source_name=nodes[source]["name"],
        source_slot=links[0]["slot_out"],
        target=target,
        target_name=nodes[target]["name"],
        target_slot=target_slot,
        hops=hops,
        folder=ctx.data_dir,
        csv=ctx.data_dir / "latency.csv",
    )


def read_lines(r: Run) -> list[dict]:
    """The sidecar lines written since the last read, each with its meta carried forward; a partial
    last line waits."""
    if r.path is None or not r.path.exists():
        return []
    with r.path.open("rb") as f:
        f.seek(r.offset)
        chunk = f.read()
    cut = chunk.rfind(b"\n")
    if cut < 0:
        return []
    r.offset += cut + 1
    lines = []
    for raw in chunk[:cut].split(b"\n"):
        if not raw:
            continue
        line = json.loads(raw)
        for key, value in line.get("meta", {}).items():
            if value is None:
                r.carried.pop(key, None)
            else:
                r.carried[key] = value
        r.frames += 1
        lines.append({"t": float(line["t"]), **r.carried})
    return lines


def trace(r: Run, source: list) -> tuple[list[float], dict] | None:
    """The hop latencies of a target frame, source to target, and the source frame's entry; None
    where its chain does not reach the source node along the path."""
    entry = source[0]
    hops: list[float] = []
    upstream = [r.source] + [h.uid for h in r.hops[:-1]]
    for hop, up in zip(reversed(r.hops), reversed(upstream)):
        held = entry.get("inputs", {}).get(hop.slot_in, [])
        nxt = next((source[p] for p in held if p < len(source) and source[p].get("node") == up), None)
        if nxt is None:
            return None
        hops.append((entry["emit"] - nxt["emit"]) * 1000.0)
        entry = nxt
    hops.reverse()
    return hops, entry


def settle(r: Run, out) -> None:
    for line in read_lines(r):
        source = line.get("source")
        index = line.get("index")
        if not isinstance(source, list) or not source or index is None:
            r.unpaired += 1
            continue
        traced = trace(r, source)
        if traced is None:
            r.unpaired += 1
            continue
        hops, origin = traced
        total = sum(hops)
        cells = [repr(line["t"]), str(index), repr(origin["time"]), str(origin["index"]), f"{total:.6f}"]
        cells += [f"{ms:.6f}" for ms in hops]
        out.write(",".join(cells) + "\n")
        for hop, ms in zip(r.hops, hops):
            hop.total_ms += ms
            hop.count += 1
        r.paired += 1
        r.last_ms = total
        r.total_ms += total
        r.min_ms = min(r.min_ms, total)
        r.max_ms = max(r.max_ms, total)
    out.flush()


async def locate(ctx, r: Run) -> None:
    """Where the recorder writes the target's stream, from its status: the sidecar is the file's
    `.jsonl`."""
    status = await ctx.call("record status")
    r.folder = Path(status["folder"])
    for s in status["streams"]:
        if s["node"] == r.target_name and s["slot"] == r.target_slot and s["file"]:
            r.path = r.folder / Path(s["file"]).with_suffix(".jsonl")
    r.csv = r.folder / "latency.csv"


async def tail(ctx, r: Run) -> None:
    try:
        with r.csv.open("w") as out:
            out.write(",".join(FIXED + [f"{h.name}_ms" for h in r.hops]) + "\n")
            out.flush()
            while not r.stopping:
                await asyncio.sleep(TAIL_EVERY)
                if r.path is None:
                    await locate(ctx, r)
                settle(r, out)
            settle(r, out)
    except Exception as e:  # noqa: BLE001 — the panel shows the cause
        r.error = str(e)
        ctx.log(f"latency run failed: {e}", level="error")


async def disarm(ctx, r: Run) -> None:
    if r.armed:
        await ctx.call("record disarm", {"output": f"{r.target}/{r.target_slot}"})
        r.armed = False


@plugin.op("start", kind="effect")
async def start(ctx, args: Start) -> dict:
    """Record the latency from `source` to `target`, each a node uid or name, summed hop by hop
    along the fewest wires between them: a recording of the target's output, with `latency.csv`
    beside it. The target's first output is timed unless a slot is named."""
    global run
    if run is not None:
        raise RuntimeError("a latency run is in progress; stop it first")
    r = await plan(ctx, args)
    try:
        armed = await ctx.call("record arm", {"output": f"{r.target}/{r.target_slot}"})
        r.armed = bool(armed.get("changed"))
        root = ctx.data_dir / "recordings"
        root.mkdir(parents=True, exist_ok=True)
        # The recorder dates the folder itself.
        await ctx.call("record start", {"root": str(root), "name": f"{r.source_name}-{r.target_name}"})
        await locate(ctx, r)
    except Exception:
        await disarm(ctx, r)
        raise
    r.task = asyncio.create_task(tail(ctx, r))
    run = r
    ctx.log(f"Timing {' → '.join([r.source_name] + [h.name for h in r.hops])} into {r.folder}")
    return r.describe(True)


@plugin.op("stop", kind="effect")
async def stop(ctx, args: Empty) -> dict:
    """End the run: the recording stops, the last ticks are settled, and the output this run
    armed is disarmed. Answers the run's summary, which `status` keeps until the next start."""
    global run, last
    r = run
    if r is None:
        raise RuntimeError("no latency run is in progress")
    run = None
    try:
        await ctx.call("record stop")
    finally:
        r.stopping = True
        if r.task is not None:
            await r.task
        await disarm(ctx, r)
    last = r
    ctx.log(f"Timed {r.paired} ticks into {r.csv}")
    return r.describe(False)


@plugin.op("status", kind="read")
async def status(ctx, args: Empty) -> dict:
    """The run in progress, or the last one: the path, where it writes, and the ticks so far."""
    if run is not None:
        return run.describe(True)
    if last is not None:
        return last.describe(False)
    return {"running": False}


@plugin.on_stop
async def stopped(ctx):
    if run is not None:
        await stop(ctx, Empty())
