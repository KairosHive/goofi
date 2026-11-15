# goofi-build

goofi-build is a program of the goofi repository, next to goofi-pipe and goofi-data
(`goofi-data.md`, ecosystem ground). It makes models from goofi-data datasets. It is not a plugin:
it runs on its own and is connected to the ecosystem through the home, the stack and the account.
Status: planning; nothing is built. goofi-data comes first (`goofi-data.md`); goofi-build is for a
later session.

## Decisions

- Builds models out of goofi-data datasets, selected by their properties: a personal model from
  one person's datasets, a modality model from a tag.
- It shares what `goofi-data.md` records as ecosystem ground: the shared base crates, the
  frontend stack and the op vocabulary, the one goofi UI library, the one goofi account and the
  remote data repository.
- The present `backend/goofi-build` crate (the pipeline that compiles a node's `.rs` file into a
  library) is renamed `gp-build` with the rename to goofi-pipe, which frees the name.

## Remaining

- The model-building flow, from a property selection to a saved model.
- The goofi-build binary and its crates in the goofi repository.
