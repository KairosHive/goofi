# goofi-data and goofi-build

Two standalone programs in repositories of their own next to goofi (`../goofi-data`,
`../goofi-build`). They are not plugins: a plugin runs attached to goofi and adds to a live patch;
these programs run on their own and are connected to goofi through its home, its stack and its
account. Status: pending. This file records the product decisions of 2026-11-06; nothing is built.

goofi records; goofi-data keeps, labels, derives and shares what was recorded; goofi-build makes
models from it. With the online node library, the three form one ecosystem: a meeting point and a
marketplace for autopoietic research and arts. The word for a standalone program of the ecosystem
is not chosen yet.

## Decisions

### Shared ground

- Both programs integrate with goofi by sharing its locations: the home `goofi_supervisor::layout`
  owns (`~/.goofi/recordings`, `config.toml`, the node roots). A recording's folder, manifest and
  sidecars (`goofi_record`) are the dataset's native form. No second copy of the data.
- Both use goofi's frontend stack (SvelteKit, `panelty`, `plotluck`) and its op vocabulary: one
  capability interface, so panels, CLI, MCP and scripts drive the same ops.
- One goofi account serves the whole ecosystem: the remote data repository, the online node
  library (upload a node to share it, download a node into goofi) and sharing between people.

### goofi-data: the dataset manager

- A panel lists and manages the datasets local to the machine: assign tags, sort, filter.
- A dataset carries the properties a model is built from: the person it was recorded from, tags
  for modalities and other labels, and its lineage. A property is set once on the dataset and
  read from there; nothing mirrors it.
- A derivatives panel creates a derivative of a dataset through one of two transformations:
  a pre-configured generator (preprocessing, data cleaning) or a goofi patch. The patch run is
  deterministic: every frame of the source goes through the patch and none is dropped. The
  result is saved as a new derivative.
- Derivatives form a tree. A derivative stores its source and its transformation (the generator
  configuration or the patch archive) beside its data, so every derivative links back to where
  its data came from.

### goofi-build: the model builder

- Builds models out of goofi-data datasets, selected by their properties: a personal model from
  one person's datasets, a modality model from a tag.

### The remote repository

- Both programs talk to one remote data repository that hosts personal data in the goofi-data
  format, shares datasets between people and holds collective datasets. The service is not chosen.
- When a person sets up a dataset or an experiment, the ecosystem suggests dataset types and tags
  from the community's collective knowledge, so the recorded data's properties align with the
  properties of data in the ecosystem and collective research across aligned data is possible.
- The suggestion must not constrain: the person expands the ecosystem's properties as freely as
  they adopt existing ones. Suggesting only what exists is the failure to design against.

## Built alongside, in goofi

These are goofi work, needed by the two programs and built with them.

- **The deterministic runtime mode.** goofi has playback nodes (`ArrayPlayback`,
  `StringPlayback`, `TablePlayback`, `goofi_core::replay`) and a harness-driven clock
  (`Clock::External`), but no mode that feeds a recording through a patch one frame at a time,
  with the patch clock driven by the recording's timeline, and records every output. It is a goofi
  op; goofi-data invokes it and does not run patches itself.
- **Account management.** The goofi account, its credentials in the home, and the ops that log in
  and out.
- **The online node library.** Upload a node bundle under the account, browse and download one
  into goofi. `library.md` owns the local library and Git sources; this is the hosted side.

## Remaining

- Create `../goofi-data` and `../goofi-build`, each a binary serving its own app the way goofi
  does, reading the home through `goofi_supervisor::layout`.
- Define the dataset record: the recording folder as it is, plus the metadata goofi-data adds
  (person, tags, lineage). Decide where that metadata lives (beside the manifest, or one catalogue
  in the home) and which program owns writes to it.
- Define the derivative record: source reference, transformation, data, in one tree.
- The dataset panels, the derivatives panel and the tag and person management in goofi-data.
- The model-building flow in goofi-build, from a property selection to a saved model.
- The remote repository, upload, download and sharing.
- The suggestion mechanism and its balance between alignment and expansion.

## Open

- **The name `goofi-build` is taken** in this workspace: `backend/goofi-build` (`goofi_build`) is
  the pipeline that compiles a node's `.rs` file into a library. Rename one before the repository
  exists.
- **The word for a standalone program of the ecosystem**, as against a plugin.
- The remote service and its credential model. `plugins.md` carries the same question for its
  session/corpus plugin, the live-patch side of the same data.
- How suggestions weigh the community's existing properties against a new one, and what signal
  from the ecosystem feeds them.
