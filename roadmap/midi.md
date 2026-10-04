# MIDI as a variable bus

MIDI is a bus: a grabbed device is a group of variables, managed in the MIDI panel, learned
over `/data/variables/<name>`. That shipped. What is left is the bridge from a variable onto a
cable. Decided 2026-10-04; builds on `state-machine.md`.

## Decisions

- **A Variable node in every engine.** Some VST3 plugins take MIDI through an input slot, and
  a cable is the only way into one. Each engine ships a `Variable` node with one param, the
  variable's name, and one output slot that carries the variable's frame whole: the node
  subscribes to the variable's wire as any consumer does and re-emits each frame on its own
  plane, on the audio plane once per block. It is the one bridge from the variable layer onto
  a cable, so a pad's sheet reaches a graphics node the same way. The reverse, a cable into a
  variable, stays the follow source.

## Remaining work

- The `Variable` node for signal, audio and graphics, with a situation that cables a `notes`
  array into a VST3 fixture and a pad into a graphics node.

## Open questions

- `midi_out.py` stays a node for now with the `mido` requirement: a MIDI sink has no place on
  an input bus. A group flagged as a sink, whose writes go to the port, is the alternative.
- Hot-plug is noticed on `midi list`, which the panel asks every two seconds while open. A port
  pulled and plugged back in while no panel is open is read again at the next list.
- The audio engine's own voice driver reads MIDI notes to drive voices. Whether it reads the
  bus through the `Variable` node (`notes` as a `[128]` array, one block behind) or keeps a
  port of its own.
