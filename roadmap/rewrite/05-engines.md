# 05. Engines

A node runs on an engine. The engine is the first half of its type: `signal:Psd`, `audio:Osc`,
`graphics:Blur`. The three engines share one graph, one node contract (part 04) and one set of
rules. A cable crosses between them through a node that declares the crossing.

| Engine | Frames of | Runs on |
| --- | --- | --- |
| signal | Array, string and table data at whatever rate the source runs. | Each node by itself, on its own events. |
| audio | Audio blocks at the device rate. | One ordered plan on the audio clock. |
| graphics | Textures. | One ordered plan on the GPU, at the frame rate. |

## 5.0 Rules for every engine

- An engine is planned from settled state only. A plan is compiled when the patch settles, with
  a cost that follows the change.
- A clock thread (the audio callback, the render tick) never allocates, never takes a lock that
  another thread can hold, and never waits for another thread. Everything crosses to it without a lock and without
  allocation. Objects are freed on a thread that may take time.
- A slow build (a device open, a plugin birth, a shader compile, a node setup) is prepared apart
  and lands whole at a settle (part 02.4).
- No op waits on a tick of an engine.
- Code of a third party is contained: a panic is caught at every boundary and becomes the error
  of the node; scans and probes run in child processes; a faulty node leaves the plan with its
  reason and returns at a restart.
- A loss is a counted gap in the frame index, never a silent hole.
- A loop needs a node that declares feedback. Feedback is declared one way in every engine.
- A plane crossing is one concept with one declaration: an input slot of a foreign kind with
  the crossing params of that kind. A tap out of a plane is one concept the same way.
- Each engine gives real-time priority to its clock thread through the foundation, meters the
  cost of each node, and reports through the one status shape (part 02.3).
- A changed node source runs after a library refresh without a restart of the program, in every
  engine.
- Each engine has a group of common params (part 04.3) declared in one place.

## 5.1 The signal engine

- A signal node runs on its own events (part 06). It never waits for another node.
- The engine sends each changed node its new wiring, sources and armed outputs at a settle.
- A node is initialized as one unit: construction, the replay of every param through the
  param-changed hook, then setup. A failure leaves the node in the setup stage with the error.
  An interaction with the node (a param edit, a pulse, a refresh) tries again at once; otherwise
  the node tries again at most once per second.
- A required input with no frame is an error of the node and the node is not called.
- A list input holds one value per cable in connection order, each with the name of its source.
- When a cable is removed, the frame it delivered goes with it.
- A node runs in one of these tiers, chosen by goofi and not by the author:
  - **Rust, in process**: a node built before the program started.
  - **Rust, hosted**: a node built after the start runs in a child process, so the newest build
    runs without a restart.
  - **Python, in process**: on the free-threaded interpreter, when the imports of the node keep
    the GIL disabled.
  - **Python, subprocess**: on the interpreter with the GIL, one process per node.
- A discovery probe imports each Python file in a real interpreter and decides its tier. The
  answer is cached by content. A failed probe makes the type unavailable, with the missing
  module or the last error line.
- An in-process Python node that enables the GIL at run time reports an error, and its type
  moves to the subprocess tier at its next restart.
- A hosted node (Rust or Python) is an independent node on the transport: it schedules itself,
  sends its frames itself, and takes its control messages from a queued connection. The main
  process manages the process: alive, and started again when it dies.
- The palette shows the tags of a node, never its tier. The inspector shows the tier.
- The expression evaluator belongs to the foundation of the engines, not to one engine. It
  compiles an expression once per source text.
- **Built-in crossings:** a signal node that takes an audio input receives blocks of
  `[channels, samples]` with `sfreq`. A signal node that takes a texture input receives pixels
  as `[height, width, 4]`, and works with no viewer open.

## 5.2 The audio engine

### 5.2.1 Clock and devices

- Audio is rendered in blocks of 64 frames. The device callback asks for any number of frames.
- The output device is the clock. The block count is the time of the audio plane; it is tied to
  the patch time at each device change, and the drift between the two is reported.
- With an external clock (tests, the Kahn mode) no device opens and the driver renders by hand.
- The rate is the default output rate of the device, and 48 000 Hz with no device. A rate change
  prepares every node again.
- Hosts: every host of the platform (ALSA, PulseAudio, JACK, WASAPI, ASIO, CoreAudio). A device
  is named `<Host>: <device>`; `default` is the default of the platform.
- The buffer size is a variable of the patch (`audio.buffer_size`). A change opens the device
  again.
- A device opens apart from the clock, with a ceiling. A device that failed is not tried again
  until its name or the buffer size changes; the previous device stays. A stream that dies is
  opened again once.
- The render uses a pool of helper threads; the stages of one level of the plan run in parallel.
- On a contended lock or a panic in a render the engine plays silence and counts it.
- The engine publishes `audio.rate`, `audio.channels`, `audio.driver`, `audio.device` and
  `audio.hosts` as variables.
- Status: clock, device, rate, channels, callbacks, underruns, contended callbacks, the longest
  render, late renders, pool waits, and per node its mean and maximum cost.

### 5.2.2 Cables and params

- An audio cable is planar float32 with N channels. A port with one channel feeds every channel.
  A channel beyond the width reads silence. A list input sums its cables. An unwired input reads
  silence and says that it is unwired.
- The width of a port follows the widest input by default; a node can compute its widths.
- Conventions: bipolar signals in [−1, 1], unipolar in [0, 1], a gate is high above 0, pitch is
  volts per octave with 0 V at C4.
- A param has one source per block:
  - a constant glides across one block; a pulse, a boolean, an integer and a string step;
  - a direct reference to an audio output is modulation at audio rate;
  - an expression is evaluated beside the clock and arrives as values per block;
  - a functional is evaluated at the time of the block and ramped;
  - a pulse is high for one block.
- A node declares which of its params take audio-rate sources.
- A value that cannot be applied is an error on the param, never a silent drop.
- **Crossing in** (an array input on an audio node): the newest frame plays as a waveform (it
  loops until the next; with mix, range, speed, a resampling method, a morph between frames and
  smoothing) or drives oscillators (frequencies, or frequencies over phases; pitch in Hz or
  volts per octave). A frame is limited to 10 s.
- **Tap out**: each output is readable from the signal plane and by viewers as frames of
  `[channels, samples]` with `sfreq`, only while a reader exists.
- **Recording tap**: each armed block is one frame with its block number, so a lost block is a
  gap.

### 5.2.3 Built-in nodes of the engine

- `AudioOut`: a list audio input, a device, a channel selection, a gain. Every `AudioOut` sums.
  The first one names the clock device; one on another device reports that the clock is
  elsewhere.
- `AudioIn`: an audio output, a device, a channel selection. It opens at the clock rate and
  reports the rates the device offers when it cannot.
- Channel selection: `all`, `1`, `1-2`, `1,3-4`, `4-3`; one-based; repeats allowed. A selection
  routes; channels not named stay untouched.
- `AudioPlayback`: plays a WAV file (a bare name resolves in the recordings folder), with a
  position, a reset and a loop.
- `MidiIn`: opens a port and deals its notes out as voices: `gate`, `pitch` and `velocity`, one
  channel per voice, with a voice count and a bend range. Voices are allocated round robin. The
  sustain pedal and the pitch wheel apply.
- The engine treats a built-in node through the node contract, never through its type name.

### 5.2.4 Polyphony

- A voice is a channel. A polyphonic patch is a patch with wide cables.

### 5.2.5 VST3

- VST3 is a format, not an engine. A plugin is a node of the audio engine. Its types come from
  the machine the patch is open on; goofi ships none.
- Scan: the plugin folders of the platform and the audio node folders of the roots. Each bundle
  is described by a child process with a ceiling. The verdict, a refusal included, is cached by
  content. A failed plugin is an unavailable type with the reason.
- One type per audio class of a bundle, with the vendor as a prefix on a name clash.
- Derived manifest: one list audio input per input bus, one output per output bus; for an
  instrument a `voice` input with gate, pitch and velocity. Params: the automatable params of
  the plugin, normalized 0 to 1, or options from the texts of the plugin for a stepped param,
  in groups from the units of the plugin.
- A gate rise sends a note; the pitch between two notes goes to per-channel pitch bend or to
  note tuning. The bend range is learned per plugin and cached with the scan.
- The editor of a plugin opens in a native window. Edits in the editor go to the patch; edits of
  the patch go to the editor. A preset change reads every value back.
- The state of a plugin is saved per node in the workspace (part 03.10).
- The engine reports the latency of a plugin.

### 5.2.6 Errors

- A watchdog removes a node from the plan when it panics, emits a non-finite value, or costs
  more than its budget for several blocks in a row. The node outputs silence and shows the
  reason until a restart.

## 5.3 The graphics engine

### 5.3.1 Device, clock, demand

- One GPU device per process. It needs no display. A software adapter is the fallback.
- Every texture is 16-bit float RGBA with linear filtering. An unwired input reads a transparent
  texture. The largest size is 8192 per axis.
- The clock ticks at `graphics.fps`. An overrun does not catch up. With an external clock the
  driver renders by hand and each tick waits for the GPU.
- A stage renders only when a reader reaches it backward over the cables: a window, a viewer, an
  armed output during a take, a crossing to another plane. This is the one place where a reader
  decides whether work is done: a stage with state advances only while it is read.
- Status: adapter, backend, clock, rate, windows, frames, stages, the longest tick, and the cost
  per node.

### 5.3.2 Node kinds

- **Shader node:** a WGSL file with a header (part 04.4). The body defines `shade(uv)` and one
  `next_<buffer>(uv)` per state buffer. The engine supplies: the patch time, the resolution, a
  sampler, the params as one struct, a frame counter (0 on fresh state), one texture per input
  by its name, and each state buffer by its name as it was at the previous tick. `uv` (0, 0) is
  the top left. Line numbers in an error match the file.
- **Texture source:** a Rust or Python node of the graphics engine with one texture output. It
  emits pixels, or a shader for the engine to render. The newest frame wins and the last texture
  stays. It has the scheduling params of a signal node.
- **State and feedback:** a state buffer is a pair of textures swapped after every render. A
  feedback node closes a loop. A size change makes new buffers and resets the frame counter.
- A shader is validated at the scan and compiled apart from the clock. A refusal is the error of
  the node.
- **Crossing in** (an array or audio input on a graphics node): the newest frame is uploaded as
  a texture: `[N]` as one row, `[H, W]` as grey, `[H, W, C]` with up to 4 channels. Its range
  and scale reach the shader. Named EEG channels on the first axis give a head layout texture.
- **Geometry:** one tiled array format carries geometry (13 kinds) from signal nodes to shaders,
  with one encoder and decoder definition.

### 5.3.3 Resolution

- Every graphics node has `width` and `height` in its common group. 0 on an axis follows the
  first wired texture input; with none, the size of the frame of a texture source or of the
  first upload; otherwise the default of the patch.
- A fullscreen window node takes the size of its monitor.

### 5.3.4 Windows

- A window node shows its input in a native window and passes it through. Params: fullscreen and
  the screen. The title is the name of the node and the measured rate. Monitors are ordered
  primary first, then left to right.
- One window loop on the main thread serves every native window: output windows and plugin
  editors. A lost display does not stop the program.
- A window is presented from the GPU directly where the platform allows, otherwise by readback.
- Without a display the engine runs headless.

### 5.3.5 Readback

- A stage is read back for a window, for a viewer (at the size the viewer wants, converted on
  the GPU) and for a recording (full size, each frame numbered and dated by its tick).
- Viewer readback runs at the viewer rate of the patch. Readback never slows the render thread.

## 5.4 Devices and capture

- Camera, video file, screen and NDI sources are nodes of the node library (part 04.9), not
  functions of an engine.
- LSL and OSC in and out, and MIDI out, are nodes of the signal engine.

## 5.5 Start screen

- While a program starts, the terminal shows a banner, one line per step and one bar per scanned
  folder, with a plain fallback for a terminal without colour.

## 5.6 Platform items carried from the present program

- Windows: a quoted agent command; the situations must run in CI; the latency floor of shared
  WASAPI; the licence of ASIO is a release question.
- macOS: the signing entitlement for audio input.
- Linux: a native PipeWire or ALSA path for short periods.

## 5.7 Open decisions

1. Process isolation of plugin processing (today only the scan is isolated).
2. A machine with no output device renders no audio: whether the engine clocks itself.
3. Drift correction between two audio devices.
4. `MidiIn`: further controllers, aftertouch and sample-accurate timing.
5. Why some commercial VST3 plugins render silence; proof that chords keep separate bends.
6. Whether late renders are shown in the interface.
7. A control-rate param that references an audio output; the width of a spectral port.
8. Not to be built: CLAP for built-ins, Python on the audio clock, a WebAssembly node tier, a
   crossfade at reload, goofi as a plugin of a DAW.
