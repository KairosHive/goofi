# Viewer modules

A viewer kind is one declaration, not five places keyed by the same string.

## Decisions

- One module per kind in `frontend/src/lib/viewers/kinds/`, exporting one `ViewerModule`: the
  settings schema, the ask as a function of size and settings, what shapes it renders, and the
  drawing. The registry (`viewers/registry.ts`) is the record of those modules over the manager's
  vocabulary, so a kind without a module is a type error.
- The wire ask is explicit per drawn dim: a cap, or `whole`. A dim a viewer names neither way it
  has no opinion on, and the fold reads nothing into the silence. The registry test checks every
  kind names every dim it draws under every menu choice.

## Open questions

- Whether a plugin may ship a viewer kind through the same module interface. The vocabulary is the
  manager's, so a plugin kind would have to declare its dtype and dims there and its behaviour in
  the client bundle; nothing is planned until a plugin asks for one.
