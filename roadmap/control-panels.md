# Control panels: what is deferred

`variables.group.element` is the one spelling an expression uses.

## Remaining

- The reference-selection flow: a right-click on a control element, and on a node output slot,
  offers **copy name** and **select for reference**; there is ONE selection in the app, not one per
  kind; a right-click on a param then offers **reference selection**, which sets `mode: expression`
  with `variables.<group>.<element>` for a control element and `mode: reference` with `node.slot`
  for an output slot. It must not be a second selection state beside the canvas's own. A long
  press is the touch door; prove it in `tests/e2e/tests/touch.spec.ts`.
- A canvas affordance that draws a control element or a reference on the canvas; one answer
  serves both (`audio-engine.md` lists the same item).

## Open

- A double-tap rename on touch is unproved; the inspector's name field is the door.

## Not to be done

- `cl()` as a second namespace for control elements: a control element IS a variable, so
  `variables.group.element` is its one spelling; a second namespace is a second owner of one idea.
