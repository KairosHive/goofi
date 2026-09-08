# Control panels: what is deferred

The control panel itself is BUILT (2026-09-05): a group of globals, each carrying a `control`
record, drawn as knobs, sliders and fields by the `control` panel type. `globals.group.element` is
the one spelling an expression uses. What is below was decided with the owner and deliberately not
built in that step.

## The reference-selection flow

Picking a control element for a param today means typing `globals.desk.level` into the expression
editor, or using its completion. The flow the owner asked for, and its decisions:

- Right-clicking a control element offers **copy name** and **select for reference**.
- Right-clicking a node OUTPUT SLOT offers the same two.
- There is ONE selection in the app, not one per kind.
- Right-clicking a param then offers **reference selection**, which writes the right thing for what
  is held: a control element sets `mode: expression` with `globals.<group>.<element>`; an output
  slot sets `mode: reference` with `node.slot`.

Two things this must not become: a second selection state beside the canvas's own, and a
touch-unreachable feature — a long press is the coarse door, and `tests/e2e/tests/touch.spec.ts` is
where that is proved.

## A canvas affordance

`param-sources.md` owns this item for a control element and a reference alike: nothing draws either
on the canvas, and one answer serves both.

## What a widget follows, and what is still open there (2026-09-05, later)

A global now carries a `source` and the manager writes it from a producer's frames; `learn` in
the widget's inspector binds the first number that moves. Open beside it:

- A source naming a node that is not there is silent: the follower has nothing to read and says
  nothing. A param's reference carries an `error` for the same case; the global's should too.
- A double-click renames a widget's label; on touch the inspector's name field is the door, and
  a double-tap is unproved.

## The drawing pad, and the turtle that reaches it (2026-09-07)

The `draw` widget was already a control kind holding a `data:image/png;base64,…` URL — a STRING,
so it crossed the wire and saved into the patch through machinery that was already there. What
landed beside it:

- **The widget has ONE colour control.** A hue wheel, a native picker and a brightness slider stood
  together and were three doors onto one colour: the wheel could not say what the picker could, so
  the two disagreed on every grey. The swatch opens the platform's picker, and size, soft, erase and
  clear stand beside it. `drawPad.ts` and its round-trip test went with the wheel, which was the
  only thing that wanted HSV.
- **`control draw` is another HAND on the pad, never a second painter.** The op parses a turtle
  script — so a refusal names the line — and broadcasts the STROKES it makes; the widget paints
  them through the very function a pointer reaches. The op writes nothing itself, so the widget's
  own commit stays the drawing's one undo step, and the reply carries `clients` because a pad
  nobody has open draws nothing. That is the cost of the rule, accepted with it.
- **Rust does the geometry, the browser does the paint.** The split was chosen against mirroring a
  renderer in two languages: `goofi_core::turtle` parses and walks the turtle — heading, pen state,
  bezier flattening — into strokes, and only the widget knows how a stroke is coloured. The wire
  between them is a stroke, which is what a mouse makes anyway.
- **Coordinates are ONE square.** Everything outside the bitmap — a pointer's position, a brush
  across, a turtle step — is said in a 1000-square, so `width 40` from the CLI is the brush the
  slider reads 40 at. The origin is the top-left with y running down, which is the row order every
  frame in goofi already has.
- **The picture leaves the pad as a FRAME.** `Drawing` is a signal node that takes the widget's
  global the way a knob's value takes one — an expression of `globals.<panel>.<pad>` — decodes the
  PNG and emits `[H, W, 4]` RGBA in 0..1; `graphics:SignalIn` is what puts it on the GPU. The
  decoder is `goofi_core::png`, which is where it had to be: `goofi-core` is the one crate every
  generated node crate already depends on. It reads what a browser canvas writes — 8 bits a
  sample, no interlacing — and refuses the rest BY NAME rather than guessing; a decoder for the
  whole format would be a library, and nothing here produces one.

Open beside it: a hand's strokes are not written back as turtle script, so a drawing made with a
mouse cannot be read or replayed as one. Both doors reaching one script is the shape the owner
wants; the renderer mirroring it implies is what stopped it here.

## Not going to happen

`cl()` was proposed and dropped. A control element IS a global, so `globals.group.element` is its
one spelling and a second namespace would have been a second owner of one idea.
