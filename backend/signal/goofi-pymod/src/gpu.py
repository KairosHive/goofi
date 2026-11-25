"""Pictures in CUDA memory, behind `goofi.Texture`.

`cuda(array)` copies a numpy array to CUDA memory and `numpy(array)` copies CUDA memory to a
numpy array, in any node. A node the graphics tick calls shares buffers with the graphics engine:
the engine sends each buffer's memory as a file descriptor over a socket and this module maps it
with the CUDA driver, so a torch tensor reads and writes the engine's own memory.
Linux and CUDA only; without them `goofi.Texture` holds CPU pixels.
"""
import os
import struct

# One message of the engine's: its kind, a buffer's id, its allocation size, and its picture's
# width, height and row pitch. An input or a target comes with the memory's file descriptor.
_MESSAGE = struct.Struct("<QQQIII4x")
_INPUT, _TARGET, _FORGET = 1, 2, 3
_HOST, _DEVICE = 1, 2
_OPAQUE_FD = 1

_lib = None
_ctx = None
_ordinal = 0
_sock = None
_bytes = b""
_fds = []
_maps = {}
_target = None


def _check(code):
    if code != 0:
        raise RuntimeError(f"CUDA error {code}")


def _driver(uuid=None):
    """The CUDA driver and the primary context of the device `uuid` names, or of device 0."""
    global _lib, _ctx, _ordinal
    if _ctx is not None:
        return _lib
    import ctypes

    lib = ctypes.CDLL("libcuda.so.1")
    _check(lib.cuInit(0))
    count = ctypes.c_int()
    _check(lib.cuDeviceGetCount(ctypes.byref(count)))
    chosen = None
    for ordinal in range(count.value):
        device = ctypes.c_int()
        _check(lib.cuDeviceGet(ctypes.byref(device), ordinal))
        held = (ctypes.c_ubyte * 16)()
        _check(lib.cuDeviceGetUuid_v2(held, device))
        if uuid is None or bytes(held) == uuid:
            chosen = (ordinal, device)
            break
    if chosen is None:
        raise RuntimeError("no CUDA device is the graphics device")
    ctx = ctypes.c_void_p()
    _check(lib.cuDevicePrimaryCtxRetain(ctypes.byref(ctx), chosen[1]))
    _lib, _ctx, _ordinal = lib, ctx, chosen[0]
    return lib


class _Current:
    """The device's primary context, current for one driver call: torch's own context."""

    def __enter__(self):
        _check(_driver().cuCtxPushCurrent_v2(_ctx))
        return _lib

    def __exit__(self, *_):
        import ctypes

        _lib.cuCtxPopCurrent_v2(ctypes.byref(ctypes.c_void_p()))


def _copy(src, src_pitch, dst, dst_pitch, row_bytes, rows, src_kind=_DEVICE, dst_kind=_DEVICE):
    """Rows of `row_bytes` from `src` to `dst`, each a host or a device address at its own pitch.

    It returns when the copy is done, and so is every torch call made before it.
    """
    import ctypes

    class Copy(ctypes.Structure):
        _fields_ = [
            ("srcXInBytes", ctypes.c_size_t), ("srcY", ctypes.c_size_t), ("srcMemoryType", ctypes.c_int),
            ("srcHost", ctypes.c_void_p), ("srcDevice", ctypes.c_ulonglong), ("srcArray", ctypes.c_void_p),
            ("srcPitch", ctypes.c_size_t),
            ("dstXInBytes", ctypes.c_size_t), ("dstY", ctypes.c_size_t), ("dstMemoryType", ctypes.c_int),
            ("dstHost", ctypes.c_void_p), ("dstDevice", ctypes.c_ulonglong), ("dstArray", ctypes.c_void_p),
            ("dstPitch", ctypes.c_size_t),
            ("WidthInBytes", ctypes.c_size_t), ("Height", ctypes.c_size_t),
        ]

    copy = Copy(srcMemoryType=src_kind, srcPitch=src_pitch, dstMemoryType=dst_kind, dstPitch=dst_pitch,
                WidthInBytes=row_bytes, Height=rows)
    if src_kind == _HOST:
        copy.srcHost = src
    else:
        copy.srcDevice = src
    if dst_kind == _HOST:
        copy.dstHost = dst
    else:
        copy.dstDevice = dst
    with _Current() as lib:
        _check(lib.cuMemcpy2D_v2(ctypes.byref(copy)))
        # A copy between two device addresses returns before it is done.
        _check(lib.cuCtxSynchronize())


class _Owned:
    """Device memory this module allocated, freed with its last reader."""

    def __init__(self, nbytes):
        import ctypes

        ptr = ctypes.c_ulonglong()
        with _Current() as lib:
            _check(lib.cuMemAlloc_v2(ctypes.byref(ptr), ctypes.c_size_t(max(nbytes, 1))))
        self.ptr = ptr.value

    def __del__(self):
        try:
            import ctypes

            with _Current() as lib:
                lib.cuMemFree_v2(ctypes.c_ulonglong(self.ptr))
        except Exception:
            pass


class Cuda:
    """CUDA memory as an array: torch, cupy and numba read `__cuda_array_interface__` with no copy.

    The memory of a texture the graphics engine sent holds that picture for the one `process`
    call; the next tick writes the next picture into it. Copy it to keep it.
    """

    def __init__(self, ptr, shape, typestr, strides=None, owner=None):
        self.__cuda_array_interface__ = {
            "shape": tuple(shape), "typestr": typestr, "data": (ptr, False), "strides": strides, "version": 3,
        }
        self._owner = owner

    def torch(self):
        """A torch tensor on this memory, with no copy."""
        import torch

        return torch.as_tensor(self, device=f"cuda:{_ordinal}")


def _is_torch(array):
    return type(array).__module__.split(".")[0] == "torch"


def on_cuda(array):
    """Whether `array` is in CUDA memory: a torch tensor on a CUDA device, a cupy array, a `Cuda`."""
    if _is_torch(array):
        return bool(array.is_cuda)
    return hasattr(array, "__cuda_array_interface__")


def _rows(spec):
    """An array in CUDA memory as rows: its address, row pitch, bytes a row and row count."""
    import numpy as np

    shape, item = tuple(spec["shape"]), np.dtype(spec["typestr"]).itemsize
    count = 1
    for axis in shape:
        count *= axis
    rows = shape[0] if len(shape) > 1 else 1
    row = count * item // max(rows, 1)
    strides = spec.get("strides")
    if strides is None:
        return spec["data"][0], row, row, rows
    packed, step = [], item
    for axis in reversed(shape):
        packed.insert(0, step)
        step *= axis
    if len(shape) < 2 or tuple(strides[1:]) != tuple(packed[1:]) or strides[0] < packed[0]:
        raise ValueError("an array in CUDA memory must be contiguous, or contiguous rows at one pitch")
    return spec["data"][0], strides[0], row, rows


def numpy(array):
    """CUDA memory copied to a numpy array of the same shape and dtype."""
    import numpy as np

    if _is_torch(array):
        array = array.detach().contiguous()
    spec = array.__cuda_array_interface__
    out = np.empty(tuple(spec["shape"]), dtype=np.dtype(spec["typestr"]))
    if out.size:
        ptr, pitch, row, rows = _rows(spec)
        _copy(ptr, pitch, out.ctypes.data, row, row, rows, dst_kind=_HOST)
    return out


def cuda(array):
    """A numpy array copied to CUDA memory of its own, as a `Cuda`."""
    import numpy as np

    array = np.ascontiguousarray(array)
    owned = _Owned(array.nbytes)
    if array.nbytes:
        _copy(array.ctypes.data, array.nbytes, owned.ptr, array.nbytes, array.nbytes, 1, src_kind=_HOST)
    return Cuda(owned.ptr, array.shape, array.dtype.str, owner=owned)


def tensor(source, device=None):
    """A torch tensor of a texture's picture: on its CUDA memory with no copy, or of its pixels."""
    import torch

    made = source.torch() if isinstance(source, Cuda) else torch.from_numpy(source)
    return made if device is None else made.to(device)


def connect():
    """Join the graphics engine, in the child of a node its tick calls.

    The engine names its socket and its device. This process joins only when its CUDA driver
    has that device: from then the engine's pictures arrive in memory both map.
    """
    global _sock
    name, device = os.environ.get("GOOFI_TEXTURES"), os.environ.get("GOOFI_TEXTURES_DEVICE")
    if not name or not device or _sock is not None:
        return
    try:
        _driver(bytes.fromhex(device))
        import socket

        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect("\0" + name)
        sock.setblocking(False)
        _sock = sock
    except Exception:
        _sock = None


def _drain():
    """Take every message the engine sent: the buffers it shares, and the ones it dropped."""
    global _bytes, _target, _sock
    if _sock is None:
        return
    import socket

    while True:
        try:
            data, fds, _, _ = socket.recv_fds(_sock, 4096, 16)
        except BlockingIOError:
            break
        if not data:
            _sock = None
            break
        _bytes += data
        _fds.extend(fds)
    while len(_bytes) >= _MESSAGE.size:
        head, _bytes = _bytes[:_MESSAGE.size], _bytes[_MESSAGE.size:]
        kind, ident, allocation, width, height, pitch = _MESSAGE.unpack(head)
        if kind == _FORGET:
            held = _maps.pop(ident, None)
            if _target == ident:
                _target = None
            if held is not None:
                with _Current() as lib:
                    lib.cuDestroyExternalMemory(held["memory"])
        else:
            _maps[ident] = _map(_fds.pop(0), allocation, width, height, pitch)
            if kind == _TARGET:
                _target = ident


def _map(fd, allocation, width, height, pitch):
    """The device address of the engine's buffer behind `fd`. CUDA owns the fd from here."""
    import ctypes

    class Handle(ctypes.Union):
        _fields_ = [("fd", ctypes.c_int), ("win32", ctypes.c_void_p * 2), ("nvSciBufObject", ctypes.c_void_p)]

    class Memory(ctypes.Structure):
        _fields_ = [("type", ctypes.c_int), ("handle", Handle), ("size", ctypes.c_ulonglong),
                    ("flags", ctypes.c_uint), ("reserved", ctypes.c_uint * 16)]

    class Buffer(ctypes.Structure):
        _fields_ = [("offset", ctypes.c_ulonglong), ("size", ctypes.c_ulonglong),
                    ("flags", ctypes.c_uint), ("reserved", ctypes.c_uint * 16)]

    memory = Memory(type=_OPAQUE_FD, size=allocation)
    memory.handle.fd = fd
    imported, ptr = ctypes.c_void_p(), ctypes.c_ulonglong()
    with _Current() as lib:
        _check(lib.cuImportExternalMemory(ctypes.byref(imported), ctypes.byref(memory)))
        _check(lib.cuExternalMemoryGetMappedBuffer(ctypes.byref(ptr), imported, ctypes.byref(Buffer(size=pitch * height))))
    return {"ptr": ptr.value, "memory": imported, "width": width, "height": height, "pitch": pitch}


def view(ident):
    """The engine's buffer `ident` as RGBA float16 `[height, width, 4]`, with no copy."""
    if ident not in _maps:
        _drain()
    held = _maps.get(ident)
    if held is None:
        raise RuntimeError("the graphics engine sent no memory for this texture")
    return Cuda(held["ptr"], (held["height"], held["width"], 4), "<f2", (held["pitch"], 8, 2))


def download(ident):
    """The engine's buffer `ident` as a numpy array, RGBA float32 `[height, width, 4]`."""
    import numpy as np

    return numpy(view(ident)).astype(np.float32)


def _picture(array):
    """A picture in CUDA memory as `[height, width, 3 or 4]`; a torch plane or one channel is grey."""
    if not _is_torch(array):
        return array
    array = array.detach()
    if array.dim() == 2:
        array = array[..., None]
    if array.dim() != 3 or array.shape[-1] not in (1, 3, 4):
        raise ValueError("texture pixels need shape [height, width, 3 or 4]")
    return array.expand(-1, -1, 3) if array.shape[-1] == 1 else array


def deliver(array):
    """What `goofi.Texture` holds for a picture in CUDA memory.

    In a node the graphics tick calls, the picture is copied, CUDA to CUDA, into the buffer the
    engine shares for the node's output: the answer is that buffer's `(id, width, height, pitch)`.
    Until that buffer has the picture's size, and in every other node, the answer is the picture
    as numpy pixels.
    """
    import numpy as np

    array = _picture(array)
    _drain()
    if _sock is not None:
        shared = array
        if _is_torch(array):
            import torch

            shared = array.to(torch.float16)
            if array.dtype == torch.uint8:
                shared = shared / 255
            if shared.shape[-1] == 3:
                shared = torch.cat([shared, torch.ones_like(shared[..., :1])], dim=-1)
            shared = shared.contiguous()
        spec = shared.__cuda_array_interface__
        shape = tuple(spec["shape"])
        if len(shape) == 3 and shape[2] == 4 and spec["typestr"] == "<f2":
            height, width = shape[0], shape[1]
            held = _maps.get(_target)
            if held is not None and (held["width"], held["height"]) == (width, height):
                ptr, pitch, row, rows = _rows(spec)
                _copy(ptr, pitch, held["ptr"], held["pitch"], row, rows)
                return (_target, width, height, held["pitch"])
            # The engine makes a target of this size for the next tick.
            _sock.setblocking(True)
            _sock.sendall(struct.pack("<II", width, height))
            _sock.setblocking(False)
    if _is_torch(array):
        import torch

        array = array if array.dtype == torch.uint8 else array.to(torch.float32)
    pixels = numpy(array)
    return pixels if pixels.dtype == np.uint8 else pixels.astype(np.float32)
