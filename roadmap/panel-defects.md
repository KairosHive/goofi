# Panel defects

Found in testing on 2026-10-04. Fix first, before the MIDI bus and the next state machine stage.

- **A control panel text box writes late.** Typing into a text widget reaches the variable only
  on blur. Every edit must write the variable at once, as a knob drag does; blur commits nothing
  new. `TextInput` commits on blur by design for the inspector, so the control panel's widget
  needs the live path, not a change to the primitive's contract.
- **A widget's red X does nothing.** In edit mode the delete button on a control panel widget
  does not remove it. Find why the click never reaches `control remove` (the board's pointer
  capture, the lift gesture, or the button under the handle) and cover it with the control
  panel Playwright session.
- **The variables panel clips instead of scrolling.** A list taller than the panel is cut off.
  The panel must scroll its list; check the scroll area against its container height and
  cover it with a layout integrity run.
