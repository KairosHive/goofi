# VST3: a plugin plays a microtonal scale only if the host knows its bend range

Pitch bend per voice channel is the mechanism; the note's `tuning` field, note expression
(`kTuningTypeID`) and MPE are honoured by none of the plugins measured. `BEND_SEMITONES = 2.0`
in `goofi-audio/src/vst3/node.rs` is an assumption, and the true range is per plugin.

## Open

- Whether a chord holds its voices' bends apart: prove two distinct pitches sounding at once.
- How to learn the range where the plugin publishes none: measure it (sound a note, read the
  pitch, apply full bend, read again) once per plugin and cache it with the scan. `MidiIn`'s
  `bend_range` param is the precedent for a declared range.

## Not to be done

- Copying the Max for Live reference (`MEME/m4l_bundle/Biotuner`): it is not MPE, so a chord
  wears the last note's bend, and its ratio-to-bend map is exact only at a 17.31-semitone range.
