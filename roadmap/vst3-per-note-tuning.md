# VST3: a plugin plays a microtonal scale only if the host knows its bend range

goofi's pitch is continuous — volts per octave, zero at C4 — and goofi's own audio nodes sound
whatever number they are given. A VST3 plugin's note is an integer, and the fraction has to travel
beside it. `note_on` in `backend/audio/goofi-audio/src/vst3/node.rs` sends it two ways; which one
the plugin honours was measured against the 16 instruments on the development machine.

## What is settled

**The note's own `tuning` field is not the mechanism.** Exact, optional, transmitted correctly, and
ignored by every one of the 16. It stays as the fallback because it costs nothing where it is
honoured, but nothing may depend on it.

**Note expression is not the mechanism either.** `kTuningTypeID` is the one per-note offset whose
support can be ASKED, and all 16 answer no to `INoteExpressionController`. The probe was written,
run, and removed; re-add it when a plugin that answers yes is in the room.

**MPE is not the mechanism.** An MPE Configuration Message (RPN 6) was ignored by both plugins it
was sent to.

**Pitch bend is the mechanism, one per voice channel** (`wheels()` asks
`getMidiControllerAssignment` per channel). **The range is the whole problem, and it is per
plugin**: `BEND_SEMITONES = 2.0` is the MIDI default and an ASSUMPTION. Vital and Diva publish a
range param (default 2); Synplant publishes none and behaves as 12; ten publish nothing bend-named,
which is a statement about their parameter list, not about whether they accept bend — a JUCE plugin
exposes MIDI mappings as parameters that never appear in the visible list.

## What is open

**Whether a chord holds its voices' bends apart.** One note is proved. Settling it needs two
distinct pitches, both confirmed sounding, in the one configuration under measurement; a test that
put both voices on one note number proved nothing and was withdrawn.

**How to learn the range where the plugin does not publish one.** Setting it is exact and covers
two of 16. Measuring it — sound a note, read the pitch, apply full bend, read it again, divide —
needs no cooperation and would cover the rest, once per plugin, cached beside everything else the
scan remembers. `MidiIn`'s `bend_range` param is the precedent for a declared range.

## Why the reference implementation is not a model

The Max for Live device this feature is measured against (`MEME/m4l_bundle/Biotuner`) is
`is_mpe: 0` and echoes the incoming channel, so every note shares one bend and a chord wears the
last note's. Its `scale 0 2 -8191 8192` maps a frequency RATIO onto full bend, exact only at a
receiver range of 17.31 semitones; against the usual 2 it renders its own -40 cents as -4.7. It
reads as working because it fails quietly, in the direction of slightly flat.
