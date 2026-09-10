"""A bundled Python node used by the plugin session."""
import goofi

class PluginEcho(goofi.Node):
    """Forward the supplied frame."""
    INPUTS = {"input": goofi.DataType.ARRAY}
    OUTPUTS = {"out": goofi.DataType.ARRAY}

    def process(self, **inputs):
        value = inputs.get("input")
        return {"out": (value.data, value.meta)} if value is not None else {}
