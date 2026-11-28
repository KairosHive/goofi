import goofi


class Producer(goofi.SignalNode):
    """A source that declares itself."""

    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PRODUCER = True

    def process(self):
        return {"out": [1.0]}
