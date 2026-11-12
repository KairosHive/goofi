# goofi-build

goofi-build is a standalone program in a repository of its own next to goofi (`../goofi-build`).
It makes models from goofi-data datasets. It is not a plugin: it runs on its own and is connected
to the ecosystem through the home, the stack and the account. Status: planning; nothing is
built. goofi-data comes first (`goofi-data.md`); goofi-build is for a later session.

## Decisions

- Builds models out of goofi-data datasets, selected by their properties: a personal model from
  one person's datasets, a modality model from a tag.
- It shares what `goofi-data.md` records as shared ground: the frontend stack and the op
  vocabulary, the one goofi UI library, the one goofi account and the remote data repository.

## Remaining

- The model-building flow, from a property selection to a saved model.
- Create `../goofi-build`.

## Open

- **The name `goofi-build` is taken** in this workspace: `backend/goofi-build` (`goofi_build`) is
  the pipeline that compiles a node's `.rs` file into a library. Rename one before the repository
  exists.
