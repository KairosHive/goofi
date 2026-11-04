"""The latency from one node to another, summed hop by hop along the wires between them, one
line per tick of the target.

Every frame carries `procstart` and `procend`, when the run that made it began and when it left,
and its `source`: per input slot, the frame it was made from, named by its node, output, index,
`procstart` and `procend` — the immediate inputs, nothing behind them. So a hop's WAIT (its
`procstart` minus the instant its input left) and its RUN (its own `procend` minus `procstart`) are
read off the hop's own frame, and a path is followed by index from one hop's recording into the one
before it. The recorder is the one path that keeps every frame, so a run arms the output of every
node after the source on the path, records them under the plugin's data folder, and tails each
sidecar as it is written: a line's `t` is its frame's `procstart`. A tick whose chain does not
reach the source is unpaired.
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
FIXED = ["target_procend", "target_index", "source_procend", "source_index", "latency_ms"]


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
    """A node after the source on the path: its uid, the slot the path enters it on, the output
    it leaves by, and its recorded frames so far, by index, read off its sidecar."""
    uid: str
    name: str
    slot_in: str
    slot_out: str
    wait_ms: float = 0.0
    run_ms: float = 0.0
    count: int = 0
    armed: bool = False
    path: Path | None = None
    offset: int = 0
    carried: dict = field(default_factory=dict)
    frames: int = 0
    lines: dict[int, dict] = field(default_factory=dict)
    newest: int = -1


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
    pending: deque = field(default_factory=deque)
    # The newest target index taken into `pending`, so a frame is judged once.
    taken: int = -1
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
            "target": {"uid": self.target, "name": self.target_name, "slot": self.target_slot, "frames": self.hops[-1].frames},
            "path": [self.source_name] + [h.name for h in self.hops],
            "hops": [
                {
                    "node": h.name,
                    "slot": h.slot_in,
                    "mean_wait_ms": h.wait_ms / h.count if h.count else None,
                    "mean_run_ms": h.run_ms / h.count if h.count else None,
                }
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
    # Each hop leaves by the wire to the next; the target leaves by the output being timed.
    hops = [
        Hop(uid=l["node_in"], name=nodes[l["node_in"]]["name"], slot_in=l["slot_in"], slot_out=nxt["slot_out"] if nxt else target_slot)
        for l, nxt in zip(links, links[1:] + [None])
    ]
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


def read_lines(h: Hop) -> list[dict]:
    """The sidecar lines a hop's recording gained since the last read, each with its meta carried
    forward and kept by its index; a partial last line waits."""
    if h.path is None or not h.path.exists():
        return []
    with h.path.open("rb") as f:
        f.seek(h.offset)
        chunk = f.read()
    cut = chunk.rfind(b"\n")
    if cut < 0:
        return []
    h.offset += cut + 1
    lines = []
    for raw in chunk[:cut].split(b"\n"):
        if not raw:
            continue
        line = json.loads(raw)
        for key, value in line.get("meta", {}).items():
            if value is None:
                h.carried.pop(key, None)
            else:
                h.carried[key] = value
        h.frames += 1
        held = {"t": float(line["t"]), **h.carried}
        index = held.get("index")
        if isinstance(index, int):
            h.lines[index] = held
            h.newest = max(h.newest, index)
        lines.append(held)
    return lines


def entry_from(line: dict, slot_in: str, upstream: str) -> dict | None:
    """The frame `line` was made from on `slot_in`, where it came from `upstream`."""
    held = line.get("source")
    if not isinstance(held, dict):
        return None
    return next((e for e in held.get(slot_in, []) if isinstance(e, dict) and e.get("node") == upstream), None)


def trace(r: Run, line: dict) -> tuple[list[tuple[float, float]], dict] | None | bool:
    """Each hop's wait and run, in ms, source to target, and the source frame's entry. None where
    the chain does not reach the source node along the path or a frame lacks its stamps; False
    while a hop's recording has not yet reached the frame the chain names."""
    hops: list[tuple[float, float]] = []
    upstream = [r.source] + [h.uid for h in r.hops[:-1]]
    for k in range(len(r.hops) - 1, -1, -1):
        hop = r.hops[k]
        entry = entry_from(line, hop.slot_in, upstream[k])
        if entry is None or not stamped(entry, "procstart"):
            return None
        hops.append(((line["t"] - entry["procend"]) * 1000.0, (line["procend"] - line["t"]) * 1000.0))
        if k == 0:
            hops.reverse()
            return hops, entry
        before = r.hops[k - 1]
        nxt = before.lines.get(entry["index"])
        if nxt is None:
            # Not recorded yet, or never: its stream is past the index without it.
            return None if before.newest > entry["index"] else False
        line = nxt


def stamped(frame: dict, start: str) -> bool:
    return all(isinstance(frame.get(k), (int, float)) for k in (start, "procend"))


def settle(r: Run, out) -> None:
    for h in r.hops:
        read_lines(h)
    target = r.hops[-1]
    r.pending.extend(line for line in target.lines.values() if line.get("index", -1) > r.taken)
    r.taken = target.newest
    held = deque()
    while r.pending:
        line = r.pending.popleft()
        if "index" not in line or not stamped(line, "t"):
            r.unpaired += 1
            continue
        traced = trace(r, line)
        if traced is False:
            held.append(line)
            continue
        if traced is None:
            r.unpaired += 1
            continue
        hops, origin = traced
        # Source to target: from the instant the source's frame left to the instant the target's did.
        total = (line["procend"] - origin["procend"]) * 1000.0
        cells = [repr(line["procend"]), str(line["index"]), repr(origin["procend"]), str(origin["index"]), f"{total:.6f}"]
        cells += [f"{ms:.6f}" for wait, run in hops for ms in (wait, run)]
        out.write(",".join(cells) + "\n")
        for hop, (wait, run) in zip(r.hops, hops):
            hop.wait_ms += wait
            hop.run_ms += run
            hop.count += 1
        r.paired += 1
        r.last_ms = total
        r.total_ms += total
        r.min_ms = min(r.min_ms, total)
        r.max_ms = max(r.max_ms, total)
    r.pending = held
    # A frame paired or given up is not looked up again: each hop keeps only what a pending
    # target frame may still name.
    for h in r.hops:
        oldest = min((line["index"] for line in r.pending if "index" in line), default=h.newest)
        for index in [i for i in h.lines if i < oldest - 1024]:
            del h.lines[index]
    out.flush()


async def locate(ctx, r: Run) -> None:
    """Where the recorder writes each hop's stream, from its status: the sidecar is the file's
    `.jsonl`."""
    status = await ctx.call("record status")
    r.folder = Path(status["folder"])
    for h in r.hops:
        for s in status["streams"]:
            if s["node"] == h.name and s["slot"] == h.slot_out and s["file"]:
                h.path = r.folder / Path(s["file"]).with_suffix(".jsonl")
    r.csv = r.folder / "latency.csv"


async def tail(ctx, r: Run) -> None:
    try:
        with r.csv.open("w") as out:
            out.write(",".join(FIXED + [f"{h.name}_{part}_ms" for h in r.hops for part in ("wait", "run")]) + "\n")
            out.flush()
            while not r.stopping:
                await asyncio.sleep(TAIL_EVERY)
                if any(h.path is None for h in r.hops):
                    await locate(ctx, r)
                settle(r, out)
            settle(r, out)
    except Exception as e:  # noqa: BLE001 — the panel shows the cause
        r.error = str(e)
        ctx.log(f"latency run failed: {e}", level="error")


async def disarm(ctx, r: Run) -> None:
    for h in r.hops:
        if h.armed:
            await ctx.call("record disarm", {"output": f"{h.uid}/{h.slot_out}"})
            h.armed = False


@plugin.op("start", kind="effect")
async def start(ctx, args: Start) -> dict:
    """Record the latency from `source` to `target`, each a node uid or name, summed hop by hop
    along the fewest wires between them: a recording of every node's output after the source, with
    `latency.csv` beside them. The target's first output is timed unless a slot is named."""
    global run
    if run is not None:
        raise RuntimeError("a latency run is in progress; stop it first")
    r = await plan(ctx, args)
    try:
        for h in r.hops:
            armed = await ctx.call("record arm", {"output": f"{h.uid}/{h.slot_out}"})
            h.armed = bool(armed.get("changed"))
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
