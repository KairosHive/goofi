import goofi


class BadTag(goofi.SignalNode):
    """Declares a tag the vocabulary does not hold."""

    TAGS = ["sparkly"]
    OUTPUTS = {"out": goofi.DataType.ARRAY}
