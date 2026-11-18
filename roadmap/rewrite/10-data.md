# 10. goofi-data

goofi-data keeps, labels, derives, records and shares datasets. It is an offline tool and a
management system. It has three cornerstones: storage, recipes and acquisition. A cornerstone is a
core function that constitutes the software and is composed with the others. Exchange is a feature
of storage. Exploration of public datasets is not a function of goofi-data: the agent searches the
existing dataset libraries on the internet. Accounts are a function of the ecosystem (part 01).

## 10.1 Storage: the dataset file

### 10.1.1 One file

- A dataset is one file. It is memory mapped and append-only.
- The file holds regions: a header; the blob region; the sample store; per embedding space a
  vector table and an index; a footer.
- The footer holds the region offsets, a checksum and the blob index. The blob index holds per
  blob: hash, offset, length, codec, and for an array blob its dtype and natural shape.
- A blob has no inline header. Its offset points at its bytes.
- To move a dataset is to move one file. One format serves local work and transfer.
- The dataset is not human readable. The ops and the export utilities do all work on it.
- Data is not compressed by default. `dataset compress` and `dataset decompress` are ops. The
  compressed file can carry a different suffix.

### 10.1.2 Blob and sample

- A blob is the bytes of a file: raw, page-aligned in the blob region, immutable once written,
  identified by the hash of its content. It carries a codec tag: `none`, or the codec of a
  compressed blob such as a video. A blob is never a sample.
- A blob with codec `none` is the raw bytes of the array in C order. With the offset, dtype and
  shape from the store, `numpy.memmap` reads it with no goofi code.
- A sample is a view on a blob: `(blob, method, extent)`, with its metadata inline. A whole file
  as a sample is the view with the full extent. An EEG epoch is `(blob, epoch, start to end)`. An
  image crop is `(blob, crop, x y w h)`.
- A sample is an atomic segment of data that serves the downstream purpose: one EEG epoch, one
  image, a video segment, an audio segment, a full recording, a set of images, a full video file.
- One blob serves many samples. A dataset holds several kinds of samples: a full audio recording
  and its chunks are all views on one blob.
- A sample can view several blobs, one per stream. Each stream has its own dtype, shape and rate.
  All streams of a sample share one time base. A studio recording of audio and EEG is one sample:
  an audio blob, an EEG blob, and one row that views both, paired and synchronized. The row's
  blob column is a list of `(stream, blob, extent)`.
- Writers write whole. A one-to-many transform whose outputs share a shape writes them as one
  blob `[n, ...]`. Each output sample is a view on index i.

### 10.1.3 The sample store

- A sample is one row of the sample store. The row holds: id (a hash of blob, method and
  extent), its streams (per stream: blob, dtype, shape, strides and extent), method, kind (eeg,
  image, audio, ...), path, and a `meta` dictionary with the whole metadata of the sample inline.
- Each kind has a schema with mandatory `meta` fields (`sfreq` for EEG, the aspect ratio for an
  image, ...). A row is validated against the schema of its kind.
- One read per sample: no second lookup comes before the bytes. The dtype decides the decoder.
- A sliding window over an uncompressed blob is offset arithmetic on the mapping. Nothing is
  materialized.
- The sample store is an embedded Arrow IPC file. It is columnar: a filter over millions of rows
  by kind and extent does not touch the blobs. pyarrow, polars and DuckDB read it from its byte
  range.

### 10.1.4 Hierarchy, groups, properties, history

- The sample layout is flat. A sample carries its path. The levels of the path are separated by
  slashes (`sub-01/eeg/...`). The level names belong to the dataset.
- A group is defined by its name and its level: the `eeg` level of every subject is one group.
  Groups across levels are linked only by an explicit link, never automatically.
- The convention serves large structured datasets and small datasets without structure, and any
  data format.
- A dataset carries the properties a model is built from: the person it was recorded from, tags
  for modalities and other labels, and its lineage. A property is set once on the dataset and
  read from there.
- A dataset contains its full history: the tree of recipes that produced it, each with its full
  declaration. Changes are tracked.
- Epochs are one derivative among many: derived samples in a group of the original recording.

### 10.1.5 Change: append, tombstone, compact

- Nothing moves and nothing is rewritten in place. A change appends a blob, appends a store
  batch, and writes a new footer last. The old footer stays valid until the new footer is
  complete. A crash leaves a readable file. Offsets never shift.
- The unit of change is the sample row. An edit is a new row plus a tombstone of the old row.
  To delete three samples writes three tombstones. To replace the data of a recording writes one
  new blob and the rows that view it.
- Reference counting is derived from the views. A blob stays while a sample views it. A blob that
  no sample views is garbage.
- A recipe applies in place when its target is the dataset it reads. It appends its outputs,
  tombstones what it replaces, and records itself in the history.
- Until `compact`, tombstoned rows and blobs stay in the file. One more append reverts an
  in-place run.
- `dataset compact` is the one operation that reclaims space. It rewrites the file without
  tombstones and without garbage blobs. It runs on demand, before a transfer or sync, or when the
  dead bytes pass a fraction of the file. It needs twice the size of the file while it runs.
- Hashes make content portable; offsets are local to a file. Two transforms that produce the same
  bytes produce one blob. The same epoch cut twice is one sample.
- A parallel run writes one file per shard. Shards merge by concatenation with rebased offsets.
- The scheduler's spill is a temporary dataset, created on demand: the blob region alone, raw
  aligned appends, written and mapped by the same code as every dataset.
- A validator checks that a file obeys the convention.

### 10.1.6 Datasets are standalone

- One dataset never depends on another. Blobs are never linked between datasets. Every dataset
  file works when every other file is deleted.
- A derivative dataset holds a copy of only what it needs. It records the operations that produced
  it from its source, without paths and without a reference to the source.

## 10.2 Storage: adapters, locations, exchange

- One dataset convention is implemented across modular storage adapters: the local file system,
  Cloudflare R2, AWS S3, GCS, Azure, Hugging Face, Google Drive, WebDAV, SFTP and SSH access to
  the datasets of a remote machine. Each adapter implements one shared abstract interface.
- Apache OpenDAL is the backend layer under the adapters. Each backend is a build feature.
- A remote dataset streams. A reader reads the footer, then the store, then the blobs it needs,
  by range.
- An adapter reports its state and its abilities in structured form. goofi-data enables and
  disables features from that report.
- goofi-data tracks several locations at once. A dataset can live in several locations.
- There is one default data directory for all datasets. A dataset at a path outside it is
  allowed.
- goofi-data is offline first. Data recorded for a remote location while offline is stored
  locally and synced when the connection returns. Updates on the remote sync the same way. Data
  that is only remote is streamed on demand and needs no sync.
- The storage view shows the unsynchronized state, as `git status` does: local data that waits
  to go out, remote data that waits to come in, and remote updates. One action starts the sync.
  Local data is removed only after it is certain that the remote holds it.
- Other formats: goofi-data imports from and exports to common dataset formats through
  standardized converters. BIDS is one import and export utility. A goofi-pipe file recording
  (part 02.9) is read in place through a format adapter.
- A remote data repository hosts personal datasets, shares datasets between people and holds
  collective datasets. Upload, download and sharing go through the goofi account.
- When a person sets up a dataset, the ecosystem suggests dataset types and tags from the
  collective knowledge of the community. The suggestion does not constrain: a person adds new
  properties as freely as they adopt existing ones.

## 10.3 Storage: embeddings and search

- A dataset declares embedding spaces by name (`clip`, `dino`). A space has a dimension, a dtype,
  a metric and the transform that produces it.
- A sample holds zero or more vectors, each in one space.
- The transform that embeds a dataset is an ordinary transform that writes into a space.
  goofi-data stores vectors and owns no model.
- Each space has one vector table: two parallel arrays, `ids [N]` (the sample hashes) and
  `vectors [N, d]`, contiguous and page-aligned. To add a space touches no sample row. To drop a
  space drops one table. To load a sample does not read its vectors.
- The vectors are directly accessible as memory-mapped arrays. `dataset space get` gives the
  offset, dtype and shape of the matrix and of the code plane. The Python package returns the
  memmap.
- The embedding transform writes the table whole, as one slab, at the end of its run. A later
  append adds a tail slab. The footer lists the slabs. `compact` merges them.
- Search, the one method:
  1. Each vector is rotated by a seeded fast Walsh–Hadamard transform with a sign flip per
     coordinate, and reduced to the sign of each coordinate: one bit per dimension, d/8 bytes.
  2. A query scans the codes (XOR and popcount, threads over chunks) and keeps the best 4R.
  3. The 4R candidates are scored again with the query at 4 bits per dimension. The best R stay.
  4. The R candidates are ranked exactly from the mapped vector table. The top k return.
- While the code plane of a space is under about 64 MB, the scan is flat. Above, the plane is
  partitioned into lists (256 per 200k vectors, scaled with N). A query scans the centroid codes,
  takes the nearest P lists, and scans those.
- The centroids are trained with e-means (reimplemented): an exponential moving average of mass
  and position per centroid, a cosine step schedule, batch and step derived from n, k and the
  epochs, split revival of unused centroids, an exact mean at the end. It has no settings.
- The index is derived: the code plane in row order plus a list id per row. It loads as it is.
  It is rebuilt from the vector table. Append encodes, routes and pushes one row. A tombstoned
  sample is skipped by the scan. `compact` regroups the plane and retrains the centroids when N
  has doubled.
- One setting: `accuracy` ∈ {fast, default, high} sets R ∈ {100, 200, 500} and P ∈ {4, 8, 16}
  per 256 lists. It is calibrated at build on held-out vectors against an exact scan.
- Size: d/8 bytes per vector for the codes (1M × 512 is 64 MB; 1M × 1536 is 192 MB), plus 2
  bytes per row and the centroids when partitioned.
- Measured 2026-11-15 on synthetic clustered data (200k vectors, d = 512, 100 queries, one core;
  a probe, not the implementation): flat scan 0.95 to 1.7 ms; partitioned query 0.1 ms; exact
  f32 scan 95 to 102 ms. Recall@10 at R = 200: 0.999 on easy data, 0.92 on hard data, 0.97 with
  the 4-bit stage, 0.999 at R = 500. e-means reached the loss of 25 Lloyd iterations in 46 s
  against 381 s and left no empty list.
- The implementation is pure Rust with no C++ dependency, under 1000 lines with its tests.
- A text query against a CLIP space needs the text encoder: a transform run on demand.

## 10.4 Recipes

### 10.4.1 The recipe

- A derivative is made by a recipe. A recipe is a graph: its nodes are transforms and its edges
  are queues. The words are *transform* and *queue*.
- Every recipe is a goofi graph (part 03) run in the Kahn mode (part 06). Transforms are nodes
  from the one node library (part 04). Queues are cables with queued delivery. The audio and
  graphics engines are nodes of the network. A patch made in goofi-pipe runs unchanged as a
  recipe.
- A transform implements an algorithm with a defined input and output format. A transform writes
  a new blob, or supplies a view on an existing blob (no copy).
- Everything is transforms and queues. The first queue holds the samples the user selected,
  possibly as tuples (pairs, triples) when one transform needs several samples. A loader
  transform feeds a queue of loaded data. Transforms and queues follow. Writer transforms write
  blobs and samples into a dataset.
- A sample is a delimited stretch of a stream. A loader emits the frames of a sample between a
  begin marker and an end marker. A writer closes a sample on the end marker. The granularity of
  the loader (frames, or the whole sample as one frame) is a setting of the recipe.
- Cardinality: one-to-one (bandpass filter); one-to-many (one stretch in, several stretches out:
  windows of a recording); many-to-one (a `zip` over streams whose stretches are paired by
  index: correlation of two samples).
- The graph is acyclic apart from declared feedback (one frame of delay; part 06.3.2). A block of the graph can repeat: n times on the same inputs with n seeds
  (n random derivative samples, for bootstrapping), or n times with its output fed back as its
  input. A fed-back repetition is unrolled before scheduling and shown collapsed.
- A stateless transform is replicated when its input queue backs up: k workers read one queue and
  the outputs are put in frame order again before the next queue. A stateful transform is not
  replicated.
- An intermediate stays in memory while it is needed and is not saved as a sample.
- A queue has a checkpoint setting: data that passes is written to disk.
- When memory runs out, the scheduler offloads intermediates it does not need soon to the spill.
- A guard surrounds every transform: a crash or an out-of-memory failure of one transform fails
  safely and is reported.
- The configurator selects procedures: move and regroup samples; apply a transform to one sample,
  to a set, or to all samples of a kind.
- The recipe view is node-based with queue lines, in the spirit of the Cooking for Engineers
  layout (ingredients on the left, each step a column over the rows it combines).
- The run view is the graph of the recipe with the fill and progress of each queue drawn on its
  edge.

### 10.4.2 The scheduler

- The scheduler works from sources to sinks.
- One worker pool serves every queue. The scheduler's levers are: the replication factor per
  transform, the depth per queue (its memory bound), and the number of network instances that run
  over different samples at once.
- The scheduler maximizes compute utilization within a memory limit. The default memory limit is
  the free RAM of the machine; the default worker limit is the number of cores. The user sets
  lower limits.
- The scheduler allocates workers from the structure of the graph and from the limits. It never
  deadlocks.
- Limits on memory, CPU and GPU change while a run executes. The scheduler adapts.
- A run is fully resumable. Checkpoints and offloaded data let a pause and resume, or an expired
  job and its restart, continue without lost progress.

### 10.4.3 Compute adapters

- Two separate dimensions: local or remote; direct or Slurm.
- Slurm is a wrapper: the scheduler of goofi-data runs inside the Slurm job. An expired job is
  restarted and continues.

## 10.5 Acquisition: the studio

- The studio records raw multimodal data in sync, with accurate timing across modalities. It
  needs no running goofi-pipe patch.
- A studio routine is a preset configuration for a regular session. A routine is linked to a
  dataset where its recordings accumulate. To start the routine of the day is one action.
- A recording modality (the input module) follows one abstract interface and supplies
  time-stamped chunks. Writing, audio and drawing are built in. Custom modalities use the same
  interface.
- A recorder type (the file type) follows one abstract interface and puts chunks into a file,
  with its own buffering, flushing and compression. A modality does not know the recorder type.
- A goofi-pipe patch is one modality, in two forms: a running patch that the studio links to, and
  a recording patch saved with the routine, for which the studio starts goofi-pipe.
- The studio records through the recorder of goofi-data (part 02.9). The dataset recorder type
  writes the dataset file directly: one blob per stream, one sample for the full recording,
  paired and synchronized, with tags and annotations, no conversion.

## 10.6 The interface

- Every action is an op (part 07). Panels, CLI, MCP, scripts and the agent use the same ops.
- A built-in agent operates the software through the ops.
- goofi-data is modular behind standard interfaces: storage adapters, compute adapters,
  recording modalities, recorder types, transforms.
- The frontend uses the shared UI library (part 09). It is a dashboard, not a panel workspace
  like goofi-pipe.
- Views of the dashboard:
  - **Overview**: all datasets across locations, with size, kinds, properties and sync state.
  - **Dataset**: one dataset: hierarchy and groups, samples, properties, embedding spaces,
    history tree, validation result.
  - **Compare**: a set of datasets side by side.
  - **Inspect**: standard metrics, and Python commands run over the data (count after a filter,
    list targeted information).
  - **Search**: nearest samples in an embedding space, from a sample or a vector.
  - **Storage**: locations, adapters, the unsynchronized state, sync.
  - **Recipe**: the configurator and the node-based recipe view.
  - **Runs**: the run view, limits, pause, resume, cancel, revert.
  - **Studio**: routines, modalities, the linked dataset, start and stop.
  - **Console** and **Agent**: as in goofi-pipe (part 08).

## 10.7 Op vocabulary

The shape and the standard verbs are the shared ones (part 07.1.4). The shared nouns are in part
07.5. A transform is a node type: `library list` and `library get` list and describe transforms.

| Noun | Verbs |
| --- | --- |
| `dataset` | `list get add edit remove status validate compact compress decompress move sync import export publish fetch share` |
| `dataset space` | `list get add remove rebuild import` |
| `sample` | `list get add remove edit search` |
| `storage` | `list add remove` |
| `recipe` | `list get add remove validate plan` |
| `recipe transform` | `add edit remove` (`edit --repeat n [--feedback]` makes a repetition block) |
| `recipe queue` | `add edit remove` |
| `run` | `start list status pause resume cancel edit revert` |
| `compute` | `list add remove` |
| `studio` | `start stop status link` |
| `studio routine` | `list add edit remove` |

`run edit` changes the limits of a running scheduler. `recipe plan` shows the unrolled graph and
the allocation the scheduler would choose. `sample search --space clip -k 10` takes a vector file
or `--like <sample id>`.

## 10.8 Open decisions

1. The name of the convention and the file suffixes, plain and compressed.
2. The hash function (BLAKE3 is the candidate), and whether the blob column of the store holds
   the hash, the offset or both.
3. The built-in chunk methods (crop, epoch, cut, ...) and how a transform declares a method.
4. Annotations: their time base and extent, and how a span survives a windowing transform.
5. Search combined with a property filter.
6. How sample markers ride on a stream (meta, or frames of their own), and how a `zip` pairs
   stretches when one side has more.
7. GPU as a scheduler resource.
8. The scheduler's cost inputs per transform, the allocation algorithm and the offload policy.
9. Whether the graph ops of a recipe use the words of goofi-pipe (`node`, `link`) with
   *transform* and *queue* as the words of the interface only, or keep their own op nouns. One
   graph model argues for one set of graph ops.
10. The remote repository service and the suggestion mechanism.
11. The op vocabulary in 10.7 is the compacted proposal of 2026-11-16 with the standard verbs of
    part 07.1.4; it is not confirmed.
12. The dashboard views of 10.6 are a proposal; how the frontend is organized is not decided.
