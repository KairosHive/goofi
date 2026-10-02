"""The latency between two nodes, one line per tick of the second: a recording of both outputs,
read as it is written.

The recorder is the one path that keeps every frame; the data plane serves viewers at a cap. So a
run arms the two outputs, starts a recording under the plugin's data folder, and tails the two
sidecars, where each line carries the patch time of the tick that made its frame. A target tick is
paired with the latest source tick at or before it, once the source has moved past it, so a line
the drain writes late cannot be missed. The pairs go to `latency.csv` beside the recording.
"""
from __future__ import annotations

import asyncio
import bisect
import json
import math
from dataclasses import dataclass, field
from pathlib import Path

from goofi_plugin import plugin

TAIL_EVERY = 0.1
COLUMNS = "target_time,target_index,source_time,source_index,latency_ms\n"


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
class End:
    uid: str
    name: str
    slot: str
    armed: bool
    path: Path | None = None
    offset: int = 0
    carried: dict = field(default_factory=dict)
    frames: int = 0
    # Ticks as (patch time, index), in the order the sidecar gave them.
    ticks: list[tuple[float, int | None]] = field(default_factory=list)

    def describe(self) -> dict:
        return {"uid": self.uid, "name": self.name, "slot": self.slot, "frames": self.frames}


@dataclass
class Run:
    source: End
    target: End
    folder: Path
    csv: Path
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
            "source": self.source.describe(),
            "target": self.target.describe(),
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


async def end_of(ctx, ref: str, slot: str | None, role: str) -> End:
    """The node `ref` names, with the output slot to arm: the given one, or its first."""
    nodes = (await ctx.call("session state"))["nodes"]
    uid = ref if ref in nodes else next((u for u, n in nodes.items() if n.get("name") == ref), None)
    if uid is None:
        raise ValueError(f"no node `{ref}` for the {role}")
    node = nodes[uid]
    info = await ctx.call("library get", {"type": node["type"]})
    outputs = list(info.get("output_slots", {}).keys())
    if not outputs:
        raise ValueError(f"`{node['name']}` has no output to time")
    if slot is None:
        slot = outputs[0]
    elif slot not in outputs:
        raise ValueError(f"`{node['name']}` has no output `{slot}`")
    return End(uid=uid, name=node["name"], slot=slot, armed=False)


def read_ticks(end: End) -> None:
    """Take the sidecar lines written since the last read; a partial last line waits."""
    if end.path is None or not end.path.exists():
        return
    with end.path.open("rb") as f:
        f.seek(end.offset)
        chunk = f.read()
    if not chunk:
        return
    cut = chunk.rfind(b"\n")
    if cut < 0:
        return
    end.offset += cut + 1
    for raw in chunk[:cut].split(b"\n"):
        if not raw:
            continue
        line = json.loads(raw)
        for key, value in line.get("meta", {}).items():
            if value is None:
                end.carried.pop(key, None)
            else:
                end.carried[key] = value
        end.frames += 1
        end.ticks.append((float(line["t"]), end.carried.get("index")))


def settle(r: Run, final: bool, out) -> None:
    """Pair every target tick the source has moved past (all of them when `final`)."""
    read_ticks(r.source)
    read_ticks(r.target)
    src = r.source
    times = [t for t, _ in src.ticks]
    done = 0
    for t_b, i_b in r.target.ticks:
        if not final and (not times or times[-1] <= t_b):
            break
        done += 1
        k = bisect.bisect_right(times, t_b) - 1
        if k < 0:
            r.unpaired += 1
            continue
        t_a, i_a = src.ticks[k]
        ms = (t_b - t_a) * 1000.0
        out.write(f"{t_b!r},{'' if i_b is None else i_b},{t_a!r},{'' if i_a is None else i_a},{ms:.6f}\n")
        r.paired += 1
        r.last_ms = ms
        r.total_ms += ms
        r.min_ms = min(r.min_ms, ms)
        r.max_ms = max(r.max_ms, ms)
        # Everything before the match can no longer be the latest for a later tick.
        if k > 0:
            del src.ticks[:k]
            del times[:k]
    del r.target.ticks[:done]
    out.flush()


async def locate(ctx, r: Run) -> None:
    """Where the recorder writes each stream, from its status: the sidecar is the file's `.jsonl`."""
    status = await ctx.call("record status")
    r.folder = Path(status["folder"])
    for end in (r.source, r.target):
        if end.path is not None:
            continue
        for s in status["streams"]:
            if s["node"] == end.name and s["slot"] == end.slot and s["file"]:
                end.path = r.folder / Path(s["file"]).with_suffix(".jsonl")
    r.csv = r.folder / "latency.csv"


async def tail(ctx, r: Run) -> None:
    try:
        with r.csv.open("w") as out:
            out.write(COLUMNS)
            out.flush()
            while not r.stopping:
                await asyncio.sleep(TAIL_EVERY)
                if r.source.path is None or r.target.path is None:
                    await locate(ctx, r)
                settle(r, False, out)
            settle(r, True, out)
    except Exception as e:  # noqa: BLE001 — the panel shows the cause
        r.error = str(e)
        ctx.log(f"latency run failed: {e}", level="error")


async def disarm(ctx, r: Run) -> None:
    for end in (r.source, r.target):
        if end.armed:
            await ctx.call("record disarm", {"output": f"{end.uid}/{end.slot}"})
            end.armed = False


@plugin.op("start", kind="effect")
async def start(ctx, args: Start) -> dict:
    """Record the latency from `source` to `target`, each a node uid or name, as a recording of
    both outputs with `latency.csv` beside it. The first output is timed unless a slot is named."""
    global run
    if run is not None:
        raise RuntimeError("a latency run is in progress; stop it first")
    source = await end_of(ctx, args.source, args.source_slot, "source")
    target = await end_of(ctx, args.target, args.target_slot, "target")
    if (source.uid, source.slot) == (target.uid, target.slot):
        raise ValueError("the source and the target are the same output")
    r = Run(source=source, target=target, folder=ctx.data_dir, csv=ctx.data_dir / "latency.csv")
    try:
        for end in (source, target):
            armed = await ctx.call("record arm", {"output": f"{end.uid}/{end.slot}"})
            end.armed = bool(armed.get("changed"))
        root = ctx.data_dir / "recordings"
        root.mkdir(parents=True, exist_ok=True)
        # The recorder dates the folder itself.
        await ctx.call("record start", {"root": str(root), "name": f"{source.name}-{target.name}"})
        await locate(ctx, r)
    except Exception:
        await disarm(ctx, r)
        raise
    r.task = asyncio.create_task(tail(ctx, r))
    run = r
    ctx.log(f"Timing `{source.name}/{source.slot}` to `{target.name}/{target.slot}` into {r.folder}")
    return r.describe(True)


@plugin.op("stop", kind="effect")
async def stop(ctx, args: Empty) -> dict:
    """End the run: the recording stops, the last ticks are paired, and the outputs this run armed
    are disarmed. Answers the run's summary, which `status` keeps until the next start."""
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
    """The run in progress, or the last one: both ends, where it writes, and the pairs so far."""
    if run is not None:
        return run.describe(True)
    if last is not None:
        return last.describe(False)
    return {"running": False}


@plugin.on_stop
async def stopped(ctx):
    if run is not None:
        await stop(ctx, Empty())
