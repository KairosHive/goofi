# MIDI as a variable bus

MIDI leaves the node graph. A device is grabbed in a MIDI panel and shows up as a group of
variables; a widget or a param binds to one of those variables the way it binds to any other.
MIDI learn listens to the MIDI groups and nothing else. Decided 2026-10-04, after the variables
producer shipped (`667b3983`); builds on `state-machine.md`.

## Decisions

- **One bus.** A grabbed device is a variable group, one group per device, flagged as a MIDI
  source by a keyword the group carries (a group flag beside its lock, not a name prefix, so a
  rename cannot lose it). Its entries are the device's state as `MidiIn` frames it today: `cc`
  a `[128]` array in 0..1, `notes` a `[128]` array of held velocities, `bend` a number in -1..1,
  `pressure` a number in 0..1, per channel where the device is grabbed per channel. The manager
  writes them through the store on the device's own thread, like the follower: no graph lock,
  no settle, no doc patch. The group's config is locked; its values are the device's.
- **A MIDI panel manages devices and nothing else.** It lists every port the host detects,
  grabbed or not, with its state (present, grabbed, gone), and grabs or releases one. A grab is
  an op (`midi grab {port, channel?}`, `midi release {group}`) and is config: it lives in the
  document, so a saved patch re-grabs its devices on load and lists one that is absent as gone,
  values held at their last frame. The panel is a panelty panel like the variables panel, one
  per layout, and works on touch and desktop in both orientations.
- **MIDI learn reads the bus.** Learn watches every MIDI group's `cc` and `notes` streams over
  `/data/variables/<name>`, waits for the first element that moves against its baseline, and
  binds the target to `variables.<device>.cc[<index>]` as the expression it is. It no longer
  scans nodes for a `midi` tag, so the node picker, the `Add a MIDI node` refusal and the
  per-node baseline go. A target is a widget's variable or a param, as today.
- **The MIDI nodes go.** `node-bundles/signal/midi_in.py` and its `midi` tag are removed with
  the `mido` requirement; a patch that read `nd('keys').cc` reads `variables.keys.cc`. The
  device enumeration and reading move to the manager (one Rust owner under
  `goofi_supervisor`, listed by `session status`, since it holds a device).

## Remaining work

- The group flag in `VariableStore`, its document field, and the `variable group` op surface.
- The device owner in the manager: enumeration, grab, release, the reading thread writing the
  store, hot-plug (gone and back), shutdown through `AppState::shutdown`.
- The ops, the vocabulary row, the schema, and the generated TS.
- The MIDI panel; MIDI learn over the bus; delete the node scan.
- Remove `midi_in.py`, the tag, and the requirement. Rewrite `tests/e2e/tests/midi-learn.spec.ts`
  over a test device (a fixture that feeds the store, never a hardware port) and extend the
  variables situation in `goofi-tests` with a grab, a frame, a save and a reload.

## Open questions

- `midi_out.py`: a MIDI sink has no place on an input bus. Keep it as a node, or make a group
  flagged as a sink whose writes go to the port.
- The audio engine's own voice driver reads MIDI notes to drive voices. Whether it reads the
  bus (`notes` as a `[128]` array variable, one block behind) or keeps a port of its own.
