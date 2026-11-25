FROM rust:1.100-bookworm@sha256:75d26c23c24cc6ea1cdabf0714a6bff8fd7bf3248716043fd7ae86dac7f41b94 AS rust
FROM lscr.io/linuxserver/webtop:ubuntu-xfce@sha256:723d89eb297e3c4096e3f96416acad3ea060e875923988ab42dee1bd974af152

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl git libegl1 libgl1 libgl1-mesa-dri \
        libudev-dev libxkbcommon-x11-0 openjdk-25-jdk pkg-config python3 \
        x11-utils xdotool zsh \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rust /usr/local/rustup /usr/local/rustup
COPY --from=rust /usr/local/cargo/bin /usr/local/cargo/bin
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/config/.cargo \
    PATH=/usr/local/cargo/bin:$PATH

WORKDIR /workspace
COPY --chown=1000:1000 Cargo.toml Cargo.lock ./
COPY --chown=1000:1000 crates/ crates/
COPY --chown=1000:1000 docs/ docs/
COPY --chown=1000:1000 original_imagej/ original_imagej/
COPY --chown=1000:1000 tools/ tools/
COPY --chown=1000:1000 docker/ docker/
RUN chown 1000:1000 /workspace
