import goofi


class Documented(goofi.Node):
    """Every param kind, each with help text, in sections shown by the params above them."""

    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {
        "kinds": [
            {
                "count": goofi.IntParam(4, 1, 8, doc="how many", options=[1, 2, 4, 8]),
                "enabled": goofi.BoolParam(True, doc="whether to run"),
            },
            {
                "gain": goofi.FloatParam(1.0, 0.0, 2.0, doc="how loud", show=("count", [4, 8])),
                "mode": goofi.StringParam("a", options=["a", "b"], doc="which mode", show=("kinds.enabled", [True])),
                "reset": goofi.PulseParam(doc="start over", show=("mode", ["b"])),
            },
        ],
        "other": {"level": goofi.FloatParam(0.5, 0.0, 1.0)},
    }

    def process(self, data):
        return {"out": data.data}
