# Control panels: what is deferred

Decided with the owner 2026-09-05. `variables.group.element` is the one spelling an expression
uses.

## The reference-selection flow

Picking a control element for a param today means typing `variables.desk.level` into the
expression editor, using its completion, or dragging a widget label onto the param. The flow the
owner asked for, and its decisions:

- Right-clicking a control element offers **copy name** and **select for reference**.
- Right-clicking a node OUTPUT SLOT offers the same two.
- There is ONE selection in the app, not one per kind.
- Right-clicking a param then offers **reference selection**, which writes the right thing for what
  is held: a control element sets `mode: expression` with `variables.<group>.<element>`; an output
  slot sets `mode: reference` with `node.slot`.

Two things this must not become: a second selection state beside the canvas's own, and a
touch-unreachable feature — a long press is the coarse door, and `tests/e2e/tests/touch.spec.ts` is
where that is proved.

## A canvas affordance

Nothing draws a control element or a reference on the canvas; one answer serves both
(`audio-engine.md` lists the same item).

## Open

- A variable `source` naming a node that is not there is silent (`goofi-graph/src/lib.rs:627`
  leaves it out and the follower re-asks). A param's reference carries an `error` for the same
  case; the variable's should too.
- A double-tap rename on touch is unproved; the inspector's name field is the door.
- A hand's strokes on the `paint` pad are not written back as turtle script, so a drawing made
  with a mouse cannot be read or replayed as one. Both doors reaching one script is the shape the
  owner wants; the renderer mirroring it implies is what stopped it. The rule that holds:
  `control paint` is another HAND on the pad, never a second painter — Rust does the geometry
  (`goofi_core::turtle`), the browser does the paint, and the wire between them is a stroke.

## Not going to happen

`cl()` was proposed and dropped. A control element IS a variable, so `variables.group.element` is
its one spelling and a second namespace would have been a second owner of one idea.
