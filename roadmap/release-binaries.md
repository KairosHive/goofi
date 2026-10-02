# Release binaries

Native installers for Linux x86_64, Windows x86_64 and macOS arm64, built by GitHub Actions on
a version tag. An installed goofi runs with no prerequisite on the machine and still compiles
Rust nodes written at run time. Laid out 2026-10-02; nothing below is built yet.

## Decisions

1. **Electron shell, electron-builder, electron-updater.** The SvelteKit app stays as it is and
   loads in an Electron window that points at the local backend. electron-builder makes NSIS
   (Windows, per-user), dmg (macOS) and deb, rpm and AppImage (Linux). The goofi binary ships
   beside the shell as the backend and as the CLI. No headless release.
2. **Two directories, never the install directory.** The program directory (per-user Programs
   on Windows, `/Applications/goofi.app`, `/opt/goofi`) is immutable; only the installer and the
   updater write it. The runtime directory (`%LOCALAPPDATA%\goofi\runtime`,
   `~/Library/Application Support/goofi/runtime`, `~/.local/share/goofi/runtime`) is curated by
   goofi, one `<version>/` at a time: Rust toolchain, zig, uv, node, ffmpeg, the Python
   installs and venvs, libpython, the vendored SDK crates, the node build target. `~/.goofi`
   keeps only user files: patches, recordings, custom nodes, plugins, config.
3. **Provisioning on first launch, not in the installer.** The installer places the program
   directory. On first launch goofi downloads the pinned tools and Python builds, creates the
   venvs, installs the shipped wheels and the bundle requirements, all into
   `runtime/<version>`, with a progress screen. The same code runs after an update and removes
   the old version. Nothing is read from PATH or the environment: goofi resolves both
   directories from its executable and the OS conventions and passes paths to every child.
4. **The frontend is built on CI and embedded**, as now. node and npm are bundled only for
   plugin frontends.
5. **Run-time Rust nodes.** The toolchain of `rust-toolchain.toml` (rustc, cargo, rust-std)
   plus a linker: zig as `-C linker` on Linux (`-target x86_64-linux-gnu.2.28`, verified here
   with no system cc) and macOS (zig's bundled libSystem stub, APSL-2.0); on Windows the
   self-contained `x86_64-pc-windows-gnu` toolchain, no zig. `cargo vendor` of the SDK
   workspace ships in the runtime, and `goofi-build` writes the source replacement, the linker
   and `--offline` into each generated crate. The node boundary is `extern "C"`, so a gnu-built
   node loads into the msvc-built host.
6. **Python.** The bundled uv installs the pinned CPython 3.14t and 3.12 builds. The binary is
   linked on CI against the same python-build-standalone release with a relative rpath
   (`$ORIGIN/../lib`, `@executable_path/../lib`); provisioning places libpython there
   (`install_name_tool -id @rpath/...` on macOS, the DLL beside the exe on Windows) and goofi
   sets `PYTHONHOME` for its own process at start.
7. **Linux audio.** Only ALSA is linked. JACK is opened at run time (already so), PulseAudio is
   pure Rust over a socket, the native PipeWire host is dropped (its binding cannot load at run
   time; PipeWire machines use the PulseAudio and JACK paths). `realtime-dbus` goes too, so the
   binary needs no system library beyond ALSA, glibc 2.28 and the GPU driver.
8. **ffmpeg.** An LGPL build per platform, no GPL parts: H.264 through OpenH264 and the
   hardware encoders (NVENC, VideoToolbox, Media Foundation, AMF, QuickSync). goofi is AGPL
   with commercial licences on request, and a GPL x264 build could not ship in the commercial
   edition. ASIO is used under its GPLv3 grant in the public build and under Steinberg's
   proprietary grant in the commercial one.
9. **Updates.** electron-updater replaces the program directory the native way; goofi then
   provisions the new runtime on first launch. `goofi update` from the CLI triggers the same
   path. goofi prints one line at start when a newer release exists, checked at most daily.
   Until macOS builds are signed, the macOS updater downloads the dmg and opens it.
10. **Signing.** Unsigned until the stable release. The workflow carries secret-gated steps
    for Developer ID plus notarization (Apple Developer Program, 99 USD/yr) and Azure Trusted
    Signing (about 10 USD/month); they skip when the secrets are absent. The release page notes
    the Gatekeeper and SmartScreen prompts.
11. **Release workflow.** `release.yml` on `v*` tags, one job per platform on `ubuntu-22.04`,
    `windows-latest`, `macos-15`: build goofi `--release` against the pinned Python, build the
    wheels (cp314t and abi3), vendor the SDK crates, resolve the tool downloads and checksums
    into the manifest the provisioner reads, build the Electron app, run the smoke test
    (install into a clean prefix, hide the host toolchains, provision, boot, build one node per
    engine), then publish the release with checksums. The tag must equal
    `[workspace.package].version`.

## Not to be done

- No cargo-dist and no Tauri: electron-builder covers the installers and the updater, and
  Chromium is the one renderer for the canvas-heavy UI.
- No static libpython: Windows extension modules link `python314t.dll` by name.
- No prebuilt node build target: cargo fingerprints are not relocatable. The first node build
  compiles the SDK tree once; provisioning may warm it in the background.
- No frontend build on the user's machine.

## Open questions

- Linux arm64 and macOS x86_64 builds (runners exist for both).
- Whether provisioning downloads the tools or the installer carries them (download keeps the
  installers small; carrying them makes the first launch work offline).

## Layout (agreed 2026-10-02)

`goofi_supervisor::layout` replaces the `home` module. Three roots, each an OS convention with
one scoping override read per call:

| root | default | override | holds |
|---|---|---|---|
| home | `~/.goofi` | `GOOFI_HOME` (parent of `.goofi`) | custom nodes, recordings, plugins, plugin data, config, recovery |
| runtime | `%LOCALAPPDATA%\goofi`, `~/Library/Application Support/goofi`, `$XDG_DATA_HOME/goofi` | `GOOFI_RUNTIME` (replaces `GOOFI_BUILD_DIR`) | below |
| session | `/tmp/goofi-system`, `C:\Temp\goofi-system` | none | unchanged |

`runtime/<version>/` is bound to one goofi version and removed whole after an update: `tools/`
(rust, zig, uv, node, ffmpeg), `python/` (the ft and gil venvs), `lib/` (libpython), `vendor/`,
`build/` (crates, target, out, probes, vst3, plugin caches), `shipped/`. `runtime/cache/` is
shared across versions: uv cache, Python installs, cargo registry. `runtime/state/` survives an
update: recent folders, the update-check stamp. `~/.goofi/system` disappears.

`layout.tool(Tool::Cargo)` returns the bundled path under `tools/` when present, else the PATH
lookup. That rule is the whole difference between the source checkout and an installed goofi:
the checkout's tools come from PATH and `goofi-init` fills its runtime from the developer's uv,
exactly as the provisioner will. The venvs leave the repo root; `PYO3_PYTHON` serves the link
step only, and goofi sets the interpreter and `PYTHONHOME` for its own process from the layout.
Shipped content stays embedded, so the program directory is not a root the core needs.

## Remaining work

1. Layout: the module above; move the venvs, build base and shipped tree into the runtime;
   Python self-configuration; tool lookup; the `GOOFI_RUNTIME` rename in tests, CI and e2e.
2. Provisioning: move venv, wheel and requirements logic from `goofi-init` into a
   `goofi-provision` crate the binary uses, driven by a pinned manifest; `goofi-init` keeps the
   development-only parts (system package checks, cargo config, target cleanup).
3. `goofi-build`: toolchain, linker, vendor source and `--offline` from the layout.
4. Audio: drop the pipewire and realtime-dbus features; confirm JACK loads at run time.
5. Electron shell under `frontend/electron` with electron-builder config and the updater; the
   CLI's `goofi update`; the start-up check.
6. CI: wheel build, vendoring, tool manifest, Electron build, smoke test, release job.
