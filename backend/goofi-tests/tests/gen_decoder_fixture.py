"""Write `fixtures/tiny_generator.onnx` — the smallest thing the `Decoder` node calls a generator.

Eight latents in, a 4x4 RGB image out through a `tanh`, so the node's whole job is testable: the
NCHW it answers, the [-1, 1] it spans, and the rows it puts first. The bias alone draws a red-to-
white ramp DOWN the rows, which is what a frame read bottom-up would get backwards.

    python -m pip install onnx numpy
    python gen_decoder_fixture.py
"""

import pathlib

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

LATENT = 8
SIZE = 4
OUT = 3 * SIZE * SIZE

bias = np.zeros((3, SIZE, SIZE), dtype=np.float32)
bias[0] = np.linspace(-3.0, 3.0, SIZE, dtype=np.float32)[:, None]
bias[1] = 0.0
bias[2] = -3.0

# Deterministic and not zero, so a latent that moves moves the picture.
weight = (np.arange(OUT * LATENT, dtype=np.float32).reshape(OUT, LATENT) % 7.0 - 3.0) * 0.1

graph = helper.make_graph(
    [
        helper.make_node("Gemm", ["z", "w", "b"], ["flat"], transB=1),
        helper.make_node("Tanh", ["flat"], ["curved"]),
        helper.make_node("Reshape", ["curved", "shape"], ["image"]),
    ],
    "tiny_generator",
    [helper.make_tensor_value_info("z", TensorProto.FLOAT, [1, LATENT])],
    [helper.make_tensor_value_info("image", TensorProto.FLOAT, [1, 3, SIZE, SIZE])],
    [
        numpy_helper.from_array(weight, "w"),
        numpy_helper.from_array(bias.reshape(OUT), "b"),
        numpy_helper.from_array(np.array([1, 3, SIZE, SIZE], dtype=np.int64), "shape"),
    ],
)
model = helper.make_model(graph, opset_imports=[helper.make_operatorsetid("", 13)])
model.ir_version = 9
onnx.checker.check_model(model)

out = pathlib.Path(__file__).parent / "fixtures" / "tiny_generator.onnx"
out.write_bytes(model.SerializeToString())
print(f"wrote {out} ({out.stat().st_size} bytes)")
print("bias alone renders rows", (np.tanh(bias[0, :, 0]) * 0.5 + 0.5).round(3))
