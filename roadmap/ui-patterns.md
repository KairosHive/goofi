# UI patterns

An audit of every panel against `$lib/ui` and `panelty`. Shipped: `FieldGrid` (the grid a block
of `Field row`s stands on, six users), `under` (where a menu drops from its button, five users),
`Catcher` (the layer under every
popover, pick list and context menu that takes the press which closes it, so that press does
nothing else; a tooltip, a combobox's list and the settings of a widget that is dragged while
they are open have none), MIDI learn and the console's run
button on the library's buttons, icons for text glyphs, and a stated reason at each native
element that stays (the score's cog, the console's filter and prompt, the rename field). This
file holds what is open. Each item below is from a read of the code; an item marked "survey" was
found by a subagent and is not checked line by line.

## Open: a merge with no decision in it

- Three disclosures are hand-made where `Disclosure` exists: a group's head in
  `VariablesPanel.svelte`, a row of `viewers/TableTree.svelte`, and `editor/MetadataField.svelte`.
  The specs select their classes (`.grp-head`, `variable-group-toggle`), so the change moves
  markup the specs name. (survey)
- The transition pill is drawn twice, by hand, at two sizes (`machine/TransitionLane.svelte`,
  `machine/TransitionEdge.svelte`): `Chip`. The envelope mark in the score is a hand-made pill:
  `Badge`. (survey)
- `ControlPanel.svelte` deletes a widget by a red cross of its own in one layout and by a danger
  `Chip` in the other, and both carry the test id `control-delete`; `control-learn` is on two
  elements too.
- `ControlPanel.svelte` drops a refused edit with `.catch(() => {})` in seven places, and
  `RecorderPanel.svelte` in one; every other panel says it with `notify().failure`.
- A slot of a list param is reordered by pointer handlers of its own (`Inspector.svelte`); every
  other drag is `beginDrag` or `draggable`. (survey)
- The inspector's param search is a native input for a reason that no longer holds:
  `TextInput onInput` gives each key. (survey)

## Open: a decision first

- **Delete.** It is a ghost `x` button (a track, a variable, a slot, a colour stop, a recording),
  a red cross and a danger chip (a widget), a danger text button (a state, a transition, an
  attribute), and a menu entry. The same `x` closes a pane. Only a track's delete asks first.
  One glyph, and one rule for when a delete asks.
- **A label beside a control, and a bar that is too narrow.** The timeline's bar labels `bpm`
  and `dur` with a class of its own; `Field` cannot, because its label is wide and its value
  wraps. A bar that does not fit wraps in the timeline, scrolls in a node-linked panel, and is
  cut in the console, and a track's head scrolls. One inline form of
  `Field`, and one answer for a bar.
- **A section.** A line between two groups of settings is written five ways and a heading over
  a group four ways (the inspector, a track's head, the state form, the metadata panel, the
  variables panel), and a head row of "title and actions" six times. One `Section` primitive.
- **Pick one of several.** `quickPick` in four places and a `ContextMenu` in three answer the
  same question; the add-node overlay is a third thing.
- **Rename in place** has five forms: `RenameField`, a pencil and an input, a double click, a
  field that is always open, and a tab's own. A bad name is marked, shown as text, or dropped.
- **A resize edge** has four forms (the side pane, a track, a column of the score, a widget's
  corner), and only the side pane's has a role and a touch size. One `ResizeEdge`.
- **A search field** is a native input in four places, three of which restate the touch font
  floor. One `SearchInput`.
- **Smaller.** A pressed state shows four ways (a variant, `aria-pressed` with a class, a
  one-segment `Segmented`, a chip). An error shows as a toast or as one of three inline
  classes. "Add" is a ghost plus, a ghost text button or a default button. An icon button in a
  bar has three sizes. Captions mix cases ("delete", "Delete…", "Hide Track"). A held finger's
  door onto a menu is wired by hand in three places.
