# What is left of the VST3 scan's cost

Measured 2026-09-06 on a Windows machine carrying ~150 VST3 bundles: **130 s cold, 62 s warm**, to
a live headless session. Neither number is the shared memory — `C:\Temp\iceoryx2` had 939 stranded
node directories at the time and reclaiming them changed nothing measurable, because a node's cache
entry is opened by name and never enumerated (`iceoryx2-windows-noise-and-leak.md` carries that
correction). The whole of both numbers is the plugin scan, and every patch LOAD pays it again:
`rescan` re-derives the registry from the roots, and the audio engine scans the platform folders on
its own account after every one.

## Settled 2026-09-07

**A cache entry is keyed by the binary's stamp, not its bytes.** `described()` read every plugin
binary in full and hashed it — gigabytes on a machine like the one above — to decide whether it
already had the answer. It now keys on the binary's path, length and mtime, which is the same "did
the source change" a rescan asks everywhere else. The property that cost buys is real but tiny: a
`.vst3` carried INSIDE a `.gfi` lands on a fresh path with a fresh mtime after every load, so it is
scanned once more per load rather than recognised by content. Nobody ships a 200 MB bundle in a
patch, and a miss is only slow, never wrong.

**A refusal is remembered as an answer is.** A plugin that crashes the scanner, hangs it or will not
load used to write nothing, so it was retried in full on every boot and every patch load — three
crashes, two load failures and a full 20 s timeout on the machine above, for ever. The cache now
holds `Result<Bundle, String>`, so a refused bundle costs one child EVER. The spawn's own failure is
the one refusal that is not remembered: it is goofi's doing rather than the plugin's.

Not open: the 20 s ceiling stays. A plugin that hangs must not hang the boot.

## Settled 2026-09-07: the scanner's identity is its SOURCES

The key carried `stamp_bytes(scanner)` — goofi's own binary's length and mtime. The rationale was
sound and was never what was wrong: *what a host reads out of a plugin is the host's answer as much
as the plugin's*, so the host belongs in the key. mtime is the wrong spelling of "the host", because
it changes on every relink of the same code, and the next boot then rescanned every plugin on the
machine. Measured consequence: `$GOOFI_HOME/.goofi/build/vst3` held **7,167 entries, 255 MB** across
four days — about fifty-seven generations of the same 125 plugins, one per rebuild.

It is the third candidate the pass before named: a hash of the scanner's OWN sources, which is what
`goofi-build`'s `SDK_HASH` already does for the node SDKs. `CARGO_PKG_VERSION` was rejected for
moving only on a release, and a content hash of the scanner binary for not being stable on Windows,
where a fresh `.pdb` timestamp lands in the image. The sources are stable across a relink of the
same code AND change when the scanning code does, which is the property both of those miss.

Measured 2026-09-07, 125 bundles, `cargo run` (debug), to a live headless session: a cold scan is
187.9 s and a warm boot 3.6 s, and the boot after a relink was paying the cold number every time.
It is 3.4 s now.

## Open: the door that retries a refusal

A plugin refused for a reason outside its own bytes — a missing VC++ runtime is the obvious one —
now stays greyed until its binary or the scanner changes, and the only recovery is deleting
`$GOOFI_HOME/.goofi/build/vst3` by hand.

Remembering a refusal also makes the 20 s ceiling PERMANENT for whatever it cut off, and a
sample-loading instrument on a cold disk sits near it: Padshop and Retrologue both time out on the
machine reported 2026-09-07. The ceiling itself stays — a plugin that hangs must not hang the boot —
but the argument that made 20 s cheap was that it was paid every time, and it no longer is.

`library refresh` is the wrong door for it, which is what the first pass got wrong: an agent calls
that op after writing a node file, and making it discard negatives would re-pay every 20 s timeout
on a routine call. The door wants to be its own word — the op saying RETRY, not the op saying scan.

## Open: a cold scan is one child at a time

`scan_dir` walks bundles in sorted order and waits for each child before spawning the next, so a
cold scan of 150 plugins is 150 serial process launches plus every plugin's own load time. The
answers are independent and the registration that follows is what needs the order, so a bounded pool
over `described()` with the registration still sequential would cut the cold number by most of it.
Worth doing only if the cold path is still felt now that it is paid once per plugin rather than once
per load.
