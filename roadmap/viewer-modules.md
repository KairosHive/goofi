# Viewer modules

A viewer kind is one declaration, not five places keyed by the same string.

## Why

On 2026-09-29 a brain viewer beside an inline line viewer drew "cannot render this shape": the
backend fold read its whole-frame ask (no reduce entry) as no opinion, and the line's caps won
(fixed in `goofi-view`, 0bac1f3f). The frontend did not cause that bug, but it made the ask hard
to audit: a kind's dtype, `draws` and `accepts` live in `vocab.ts`, its ask in an if-chain in
`capacity.ts`, its resolution in `kind.ts`, its settings in `settingsSchema.ts`, and its drawing
in a class or component. Nothing forces the ask, the settings and the drawing to agree, and a
new kind or a new dimensionality touches every one of them.

## Decisions

- One module per kind, exporting one object: the declaration (dtype, draws, accepts, doc), the
  ask as a function of size and settings, the settings schema, and the drawing. The registry is
  the list of those modules; `VIEWER_KINDS`, `capacity.ts`, `kind.ts` and `settingsSchema.ts`
  read from it or go.
- The wire ask is explicit per drawn dim: a cap, or `whole`. The backend never interprets an
  absent entry; the `contracts` situation checks every kind states an ask for every dim it draws,
  the way it already checks `accepts` covers `draws`.
- The ask and the drawing share one source for what a kind draws at each dimensionality, so the
  `describeSpec` preview (accepted but not drawn) is derived, not restated.

## Remaining

- Write the module interface and move `line`, `image`, `brain`, `string`, `table` into it.
- Make `reduce` entries mandatory per drawn dim in `goofi_view::ViewSpec` and the frontend type;
  update the fold and its situation (`running::many_viewers…`) to the explicit `whole`.
- Extend `contracts` to the per-dim ask.

## Open questions

- Whether a plugin may ship a viewer kind through the same module interface.
