# VST3: the IK T-RackS suite renders silence

The 39 IK Multimedia T-RackS plugins accept every host call, return `kResultOk`, and write
silence into the output buffer; a JUCE plugin given the same buffers through the same code renders.

## Remaining

- Confirm on the machine that holds the suite whether the current bus order
  (`activate_buses` after `setupProcessing`, `goofi-audio/src/vst3/node.rs`) makes T-RackS
  render. If it does, delete this entry.

## Open

- Why an IK plugin mutes itself. Ruled out: parameter values, the bypass parameter, bus
  arrangement, the activation lifecycle, the host name, and output-buffer routing. What remains
  is what the plugin reads back from the host.
