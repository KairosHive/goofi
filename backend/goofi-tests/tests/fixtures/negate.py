import goofi


class Negate(goofi.SignalNode):
    """Negate the input."""

    INPUTS = {"data": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}
