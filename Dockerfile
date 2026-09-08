# goofi as one public container, in demo mode.
#
# The build path IS the run path: `goofi_init::repo_root()` is baked in at compile time from
# CARGO_MANIFEST_DIR, so the binary looks for its two venvs where the build left them. Both stages
# are /app, and the runtime stage carries the venvs and the interpreter they point at.

FROM rust:1.97.1-bookworm AS build

# uv and npm are the two tools goofi-init demands; the four -dev libraries are its `AUDIO_LIBS`,
# one per cpal host, compiled in whether or not a demo ever opens a device. `libclang-dev` is
# bindgen's, which `libspa-sys` runs: the pipewire host cannot build without it. Node comes from
# NodeSource, not apt: bookworm ships 18.20 and the frontend's vite asks for ^20.19 || >=22.12,
# so an apt node fails the SPA build — which `cargo build` refuses to fall back from. 24 rather
# than 22, matching CI: the lockfile is gitignored, and npm 10 crashes on this manifest without one.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg pkg-config libclang-dev \
        libasound2-dev libpipewire-0.3-dev libjack-jackd2-dev libdbus-1-dev \
    && curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*
RUN curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh

# The recording every example follows, warmed here rather than by the first visitor: the node
# would fetch these 31 MB on a background thread and play nothing until they landed. Above the
# source copy, so an edit never re-downloads it. The URL is `node-bundles/eeg/eeg_playback.py`'s —
# a stale copy costs the download, never the patch.
RUN mkdir -p /samples && curl -fsSL -o /samples/eeg-rest-srm.edf \
        https://s3.amazonaws.com/openneuro.org/ds003775/sub-001/ses-t1/eeg/sub-001_ses-t1_task-resteyesc_eeg.edf

# A named directory rather than uv's default under HOME, so the runtime stage copies one known path.
ENV UV_PYTHON_INSTALL_DIR=/opt/uv-python

WORKDIR /app
COPY . .

# The one setup step, then the build. goofi-init makes both venvs, installs both wheels and the
# frontend's dependencies; the frontend and every shipped node are compiled INTO the binary.
RUN cargo run -p goofi-init
RUN cargo build --release -p goofi-cli

FROM debian:bookworm-slim

# The runtime halves of the build stage's audio libraries: cpal links all four, so the binary
# needs them present to start even where no device is ever opened.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libasound2 libpipewire-0.3-0 libjack-jackd2-0 libdbus-1-3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /opt/uv-python /opt/uv-python
COPY --from=build /app/.gfivenv /app/.gfivenv
COPY --from=build /app/.gfivenv-ft /app/.gfivenv-ft
COPY --from=build /app/target/release/goofi /usr/local/bin/goofi
# The patches `--load` names. One image carries every example; the variable picks which one.
COPY --from=build /app/examples /app/examples

# The embedded interpreter is linked against the free-threaded build, whose shared library lives
# with the interpreter rather than on the loader's default path.
RUN printf '/opt/uv-python/*/lib\n' > /etc/ld.so.conf.d/uv-python.conf && ldconfig

# Sessions and any saved patch belong on a mounted volume, not in the layer.
ENV GOOFI_HOME=/data
ENV GOOFI_DEMO=1
RUN mkdir -p /data
COPY --from=build /samples /data/.goofi/data/samples

# `--port` rather than a PORT variable goofi would have to know the name of.
CMD ["sh", "-c", "exec goofi serve --bind 0.0.0.0 --port ${PORT:-8000}"]
