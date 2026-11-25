# goofi as one public container, in demo mode.
#
# The build path IS the run path: goofi-init writes the interpreter's library directory into the
# binary's rpath, and the binary looks for its two venvs in its runtime. Both stages name one
# runtime, /opt/goofi, and the runtime stage carries the venvs and the interpreters they point at.

FROM rust:1.97.1-bookworm AS build

# uv and npm are the two tools goofi-init demands; the -dev libraries are its `AUDIO_LIBS`, one per
# cpal host, and dbus for the real-time grant, compiled in whether or not a demo ever opens a
# device. Node comes from NodeSource, not apt: bookworm ships 18.20 and the frontend's vite asks
# for ^20.19 || >=22.12, so an apt node fails the SPA build, which `cargo build` refuses to fall
# back from. 24 rather than 22, matching CI: the lockfile is gitignored, and npm 10 crashes on
# this manifest without one.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg pkg-config \
        libasound2-dev libjack-jackd2-dev libdbus-1-dev \
    && curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*
RUN curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh

# The recording every example follows, warmed here rather than by the first visitor: the node
# would fetch these 31 MB on a background thread and play nothing until they landed. Above the
# source copy, so an edit never re-downloads it. The URL is `node-bundles/eeg/eeg_playback.py`'s:
# a stale copy costs the download, never the patch.
RUN mkdir -p /samples && curl -fsSL -o /samples/eeg-rest-srm.edf \
        https://s3.amazonaws.com/openneuro.org/ds003775/sub-001/ses-t1/eeg/sub-001_ses-t1_task-resteyesc_eeg.edf

# One named runtime rather than the platform's data directory under HOME: the interpreters, the
# venvs and the build caches are made here, and the runtime stage copies this one path.
ENV GOOFI_RUNTIME=/opt/goofi

WORKDIR /app
COPY . .

# The one setup step, then the build. goofi-init makes both venvs, installs both wheels and the
# frontend's dependencies; the frontend and every shipped node are compiled INTO the binary.
# `--demo` installs no package of a bundle that `demo.toml` withholds: gigabytes the image saves.
RUN cargo run -p goofi-init -- --demo
RUN cargo build --release -p goofi-cli

# What the binary needs of the runtime is the venvs and the interpreters. The node build cache and
# the package caches stay behind: a venv's files are links into the uv cache, and outlive it.
RUN rm -rf /opt/goofi/*/build /opt/goofi/cache/uv /opt/goofi/cache/cargo

FROM debian:bookworm-slim

# The runtime halves of the build stage's libraries: cpal links both audio hosts, so the binary
# needs them present to start even where no device is ever opened. Mesa's lavapipe is the Vulkan
# device of a machine with no GPU: the graphics engine takes it as its software adapter.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates git libasound2 libjack-jackd2-0 libdbus-1-3 \
        libvulkan1 mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
# Startup checks node requirements with the same uv used to provision the image.
COPY --from=build /usr/local/bin/uv /usr/local/bin/uv
RUN uv --version
ENV GOOFI_RUNTIME=/opt/goofi
COPY --from=build /opt/goofi /opt/goofi
COPY --from=build /app/target/release/goofi /usr/local/bin/goofi
# The patches `--load` names. One image carries every example; the variable picks which one.
COPY --from=build /app/examples /app/examples

# Sessions and any saved patch belong on a mounted volume, not in the layer.
ENV GOOFI_HOME=/data
ENV GOOFI_DEMO=1
RUN mkdir -p /data
COPY --from=build /samples /data/.goofi/data/samples

# `--port` rather than a PORT variable goofi would have to know the name of.
CMD ["sh", "-c", "exec goofi --bind 0.0.0.0 --port ${PORT:-8000}"]
