import goofi


class BadSection(goofi.Node):
    """Declares one param name in two sections of a group."""

    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PARAMS = {"kinds": [{"gain": goofi.FloatParam(1.0, 0.0, 2.0)}, {"gain": goofi.FloatParam(1.0, 0.0, 2.0)}]}
