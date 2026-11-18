# 11. goofi-build

goofi-build makes models from goofi-data datasets. It is a program of the ecosystem, not a
plugin. It is specified after goofi-data works; this part holds what is decided.

- It selects datasets by their properties: a personal model from the datasets of one person, a
  modality model from a tag.
- It reads datasets through the dataset file of the goofi layer (part 10.1): the same file, the
  same memory-mapped access to samples and to embedding vectors.
- It shares the goofi layer, the shared UI library, the op vocabulary shape, the account and the
  remote data repository.
- The name `goofi-build` belongs to this program. The builder of the node library is a part of
  the node library (part 04.6) and carries no program name.

## 11.1 Open decisions

1. The flow from a selection of properties to a saved model.
2. Where a model is stored and how it is shared.
3. How a model becomes a node of the node library.
