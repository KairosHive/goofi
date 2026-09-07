# Cross-platform audit, 2026-09-06

Three finders over Windows, macOS and what is written once but behaves differently. Two rounds are
done; what is LEFT is here, because a finding nobody records is a finding nobody acts on.

The one fact behind most of it: **nothing in this tree has run on a Windows or a macOS machine.**
The suite's window host is screenless on every platform, so the whole `present` path — three
platform bodies and the swizzle — has never executed anywhere. CI compiles it and no test calls it.

## Open

- **The process is DPI-unaware on Windows**, and making it aware is TWO halves. Nothing embeds a
  manifest or calls `SetProcessDpiAwarenessContext`, so DWM stretches every window on a scaled
  display and `Screen::present`'s "one texel to one pixel" is false. The call alone is not the fix:
  a DPI-aware host must also tell each plugin its scale through
  `IPlugViewContentScaleSupport::setContentScaleFactor`, or a crisp editor comes back at a third of
  its size. Both halves need a scaled Windows display to judge, which nothing here has.
- **The graphics suite needs an adapter on all three runners and CI provisions one on Linux only.**
  Reading says macOS answers with Metal and Windows with the DX12 WARP software adapter, so the job
  is green by the runner images alone and nothing in the tree says so. WARP also renders every
  shipped `.wgsl` in software against the 240-minute ceiling.
- **A node file's extension is matched case-sensitively**, so a Windows author's `Node.PY` is
  invisible with no message. Left because goofi writes the extension itself at every door it owns,
  and because the real finding underneath is that four sites — `scan.rs`, `bridge/lib.rs`, two build
  scripts — each spell "is this a `.rs`" for themselves. One owner first, then the folding.
- **`Loop::open()` cannot fail on Windows**, so the "the display is gone" state is unreachable
  there; the agent e2e scenario is POSIX-only by construction; `patchfile.rs` writes an unquoted
  filename into `Content-Disposition` — now sanitized, but the header still has no `filename*`
  form, so a non-ASCII patch name reaches the browser mangled.

## Windows plugin hosting, from a user's report 2026-09-07

31 of a user's VST3 bundles were unavailable at once, in three shapes: `LoadLibraryExW failed`, the
scanner exiting `-1073741819` (`0xC0000005`, an access violation), and the scanner not answering in
20 s. Three causes were found by reading and are now fixed, none of them yet judged on a Windows
machine — the suite's own fixture plugin is a goofi `cdylib` with no dependency beside it and no COM
in it, so CI compiles these paths and proves nothing about them.

- **The plugin was loaded with no `LOAD_WITH_ALTERED_SEARCH_PATH`**, so a bundle whose dependency
  DLLs sit beside its binary could not find them: the default order searches goofi's OWN folder.
  The unix arm has always named its flags; the Windows arm took libloading's plain `Library::new`.
- **libloading's Windows error Displays "LoadLibraryExW failed" and nothing else** — the OS error is
  in the `source()` beneath it, which is why the report could not say whether the module was missing
  or the image was the wrong architecture. The cause chain is now rendered. The unix arm never had
  this: `DlOpen` Displays the `dlerror` string.
- **No thread that loads a plugin opened a COM apartment.** A Windows plugin that reaches COM on an
  uninitialized thread gets a null interface back from a call whose HRESULT it does not read, and
  dereferences it — which is what an access violation inside a plugin's own `initialize` looks like.
  `goofi_window::open_com_apartment` now runs on the window loop's thread and in the scanner child.

Open:

- **The scanner pumps no messages.** A plugin whose `initialize` creates a window or a timer and
  waits on it blocks until the 20 s ceiling. That is a candidate cause of the two Steinberg
  instruments that timed out, and it is not cheap to fix: the scan is one blocking call, so a pump
  means the describe runs off the loop thread that a JUCE plugin refuses to be loaded from.
- **The IK T-RackS silence** (`vst3-commercial-silence.md`) was measured before any of this. If that
  measurement was on Windows, it is worth re-taking now that an apartment is open.

## Judged and NOT a defect

- **Byte order.** `goofi-codec` states little-endian and writes `to_le_bytes`; the numpy ingest
  rejects `>`; WAV and NPY specify LE. The GPU sites use the wire's spelling for native memory,
  which is only cosmetically wrong. No `bytemuck`, no `align_to`, no buffer transmute, so aarch64
  has no alignment fault waiting either.
- **iceoryx2 on macOS**: the 31-character `shm_open` limit does not bite — the macOS PAL generates a
  short name and keeps the mapping in a state file, so the long derived service names pass.
- **The `win.rs` present path type-checks** against windows-sys 0.61.2 term for term, including the
  negative `biHeight` for a top-down DIB and the DWORD-aligned scan lines.
- **Every selector and enum value in `mac.rs`** matches `objc2-app-kit`'s own bindings, and objc2's
  encoding assertions accept the integer widths used.
- **`shared()` caches a device failure for the process life**, and that is right: no adapter appears
  while goofi runs, and re-asking would pay the refusal at every node.
- **The embedded asset tables are sorted.** `embed_spa` sorts after its walk and `files_under`
  sorts before it returns; a finder read the `read_dir` inside and not the `sort` after.
