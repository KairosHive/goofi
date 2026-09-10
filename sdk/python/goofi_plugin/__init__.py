"""Define goofi plugin operations and operation hooks."""
from __future__ import annotations

import asyncio
import contextvars
import dataclasses
import inspect
import json
from pathlib import Path
import sys
import types
import typing


_current = contextvars.ContextVar("goofi_call", default={})
_output = sys.stdout
_pending = {}
_next_id = 0


def _send(value):
    _output.write(json.dumps(value, allow_nan=False) + "\n")
    _output.flush()


def _value(value):
    if dataclasses.is_dataclass(value):
        return dataclasses.asdict(value)
    return value


def _decode(annotation, value):
    if annotation in (inspect.Signature.empty, typing.Any):
        return value
    if dataclasses.is_dataclass(annotation):
        if not isinstance(value, dict):
            raise ValueError("expected an object")
        hints = typing.get_type_hints(annotation)
        fields = {field.name: field for field in dataclasses.fields(annotation)}
        if value.keys() - fields.keys():
            raise ValueError("unknown arguments: " + ", ".join(value.keys() - fields.keys()))
        return annotation(**{key: _decode(hints[key], item) for key, item in value.items()})
    origin = typing.get_origin(annotation)
    args = typing.get_args(annotation)
    if origin in (typing.Union, types.UnionType):
        for option in args:
            try:
                return _decode(option, value)
            except (TypeError, ValueError):
                pass
        raise ValueError("value does not match the declared type")
    if origin is list:
        if not isinstance(value, list):
            raise ValueError("expected a list")
        return [_decode(args[0], item) for item in value]
    if origin is dict:
        if not isinstance(value, dict):
            raise ValueError("expected an object")
        return {_decode(args[0], k): _decode(args[1], v) for k, v in value.items()}
    if annotation is float and type(value) in (int, float):
        return float(value)
    if annotation in (str, int, float, bool, dict, list, type(None)):
        if type(value) is not annotation:
            raise ValueError(f"expected {annotation.__name__}")
        return value
    raise TypeError(f"unsupported schema type: {annotation}")


def _schema_type(annotation):
    return {str: "string", int: "int", float: "float", bool: "bool"}.get(annotation, "json")


class Call:
    """A validated operation request. Return a patch to change its arguments."""
    def __init__(self, data):
        self.op = data["op"]
        self.args = data["args"]
        self.actor = data["actor"]

    def patch(self, **arguments):
        return arguments

    def reject(self, message):
        raise ValueError(message)


class Outcome(Call):
    """The effective request and its completed result or error."""
    def __init__(self, data):
        super().__init__(data)
        self.ok = data["ok"]
        self.result = data.get("result")
        self.error = data.get("error")


class Context:
    """Runtime access to goofi operations, logging, and plugin paths."""
    def __init__(self, data):
        self.plugin_id = data["id"]
        self.package_dir = Path(data["package_dir"])
        self.data_dir = Path(data["data_dir"])
        self.cache_dir = Path(data["cache_dir"])

    async def call(self, operation, arguments=None):
        global _next_id
        _next_id += 1
        request_id = _next_id
        future = asyncio.get_running_loop().create_future()
        _pending[request_id] = future
        _send({"call": request_id, "op": operation, "args": arguments or {}, **_current.get()})
        try:
            return await future
        finally:
            _pending.pop(request_id, None)

    def log(self, message, level="info"):
        if level not in ("info", "warning", "error"):
            raise ValueError("log level must be info, warning, or error")
        _send({"call": 0, "op": "log write", "args": {
            "text": str(message), "level": level, "component": f"plugin:{self.plugin_id}"
        }, **_current.get()})


class Plugin:
    """Register one package's operations and hooks."""
    def __init__(self):
        self._ops = {}
        self._hooks = {"pre_op": {}, "post_op": {}}
        self._start = None
        self._stop = None

    def op(self, name, *, kind="effect"):
        def register(fn):
            if kind not in ("read", "effect"):
                raise ValueError("plugin operations must be read or effect")
            if name in self._ops:
                raise ValueError(f"duplicate operation: {name}")
            self._ops[name] = (fn, kind)
            return fn
        return register

    def _hook(self, phase, operation):
        def register(fn):
            if operation in self._hooks[phase]:
                raise ValueError(f"duplicate {phase} hook: {operation}")
            self._hooks[phase][operation] = fn
            return fn
        return register

    def pre_op(self, operation):
        return self._hook("pre_op", operation)

    def post_op(self, operation):
        return self._hook("post_op", operation)

    def on_start(self, fn):
        if self._start is not None:
            raise ValueError("duplicate on_start")
        self._start = fn
        return fn

    def on_stop(self, fn):
        if self._stop is not None:
            raise ValueError("duplicate on_stop")
        self._stop = fn
        return fn

    def _describe(self):
        ops = []
        for name, (fn, kind) in self._ops.items():
            hints = typing.get_type_hints(fn)
            annotation = hints.get("args")
            if not dataclasses.is_dataclass(annotation):
                raise TypeError(f"{name}: args must be a typed dataclass")
            fields = []
            for field in dataclasses.fields(annotation):
                required = field.default is dataclasses.MISSING and field.default_factory is dataclasses.MISSING
                ty = typing.get_type_hints(annotation)[field.name]
                fields.append(f"{field.name}:{_schema_type(ty)}{'!' if required else ''}")
            ops.append({"name": name, "kind": kind, "args": " ".join(fields), "doc": inspect.getdoc(fn) or "", "result": str(hints.get("return", "json"))})
        return {"ops": ops, **{phase: list(hooks) for phase, hooks in self._hooks.items()}}


plugin = Plugin()


async def _invoke(fn, *args):
    if fn is None:
        return None
    result = fn(*args)
    return await result if inspect.isawaitable(result) else result


async def _serve(config):
    ctx = Context(config)
    tasks = set()
    stopping = asyncio.Event()
    closing = False

    async def handle(message):
        nonlocal closing
        token = _current.set({"actor": message.get("actor", "plugin"), "chain": message.get("chain", [])})
        try:
            method = message["method"]
            data = message.get("data", {})
            if method == "start":
                result = await _invoke(plugin._start, ctx)
            elif method == "stop":
                closing = True
                active = [task for task in tasks if task is not asyncio.current_task()]
                for task in active:
                    task.cancel()
                await asyncio.gather(*active, return_exceptions=True)
                result = await _invoke(plugin._stop, ctx)
            elif method == "op":
                fn, _ = plugin._ops[data["name"]]
                hints = typing.get_type_hints(fn)
                args = _decode(hints["args"], data["args"])
                result = await _invoke(fn, ctx, args)
                if "return" in hints:
                    _decode(hints["return"], _value(result))
            else:
                fn = plugin._hooks[method][data["op"]]
                result = await _invoke(fn, ctx, Call(data) if method == "pre_op" else Outcome(data))
            _send({"id": message["id"], "result": _value(result)})
        except Exception as error:
            _send({"id": message["id"], "error": f"{type(error).__name__}: {error}"})
        finally:
            _current.reset(token)
            if message["method"] == "stop":
                stopping.set()

    while not stopping.is_set():
        line = await asyncio.to_thread(sys.stdin.readline)
        if not line:
            break
        message = json.loads(line)
        if "reply" in message:
            future = _pending.get(message["reply"])
            if future is not None and not future.done():
                if "error" in message:
                    future.set_exception(RuntimeError(message["error"]))
                else:
                    future.set_result(message.get("result"))
        elif closing:
            _send({"id": message["id"], "error": "plugin service is shutting down"})
        else:
            task = asyncio.create_task(handle(message))
            tasks.add(task)
            task.add_done_callback(tasks.discard)
    for task in tasks:
        task.cancel()
    await asyncio.gather(*tasks, return_exceptions=True)


def _main():
    import importlib.util
    config = json.loads(sys.argv[1])
    # User prints go to the log stream; stdout is reserved for the protocol.
    sys.stdout = sys.stderr
    sys.path.insert(0, str(Path(config["package_dir"]) / "backend"))
    path = Path(config["package_dir"]) / "backend" / "plugin.py"
    spec = importlib.util.spec_from_file_location("goofi_plugin_backend", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    _send({"ready": plugin._describe()})
    asyncio.run(_serve(config))
