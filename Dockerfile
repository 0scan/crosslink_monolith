# syntax=docker/dockerfile:1.7

ARG RUST_VERSION=1.94.0

FROM rust:${RUST_VERSION}-trixie AS builder

RUN apt-get -qq update && \
    apt-get -qq install -y --no-install-recommends \
        clang \
        cmake \
        libclang-dev \
        pkg-config \
        protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

ENV CARGO_INCREMENTAL=0 \
    CARGO_TARGET_DIR=/workspace/target

# The root build context is required because Zebra uses local path dependencies
# from librustzcash, tenderlink, wallet, zebra-gui, clay-rs, and patches.
COPY . .

RUN --mount=type=cache,id=crosslink-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=crosslink-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=crosslink-target,target=/workspace/target,sharing=locked \
    cargo build \
        --manifest-path zebra-crosslink/Cargo.toml \
        --locked \
        --release \
        --package zebrad \
        --bin zebrad \
        --features indexer && \
    cp /workspace/target/release/zebrad /usr/local/bin/zebrad

FROM debian:trixie-slim AS runtime

ARG UID=10001
ARG GID=10001

ENV UID=${UID} \
    GID=${GID} \
    USER=zebra \
    HOME=/home/zebra \
    CONFIG_FILE_PATH=/etc/zebra/zebrad.toml

RUN apt-get -qq update && \
    apt-get -qq install -y --no-install-recommends \
        adduser \
        ca-certificates \
        curl \
        libgcc-s1 \
        libstdc++6 \
        util-linux \
    && addgroup --quiet --gid "${GID}" zebra \
    && adduser --quiet --gid "${GID}" --uid "${UID}" \
        --home "${HOME}" zebra --disabled-password --gecos "" \
    && mkdir -p /etc/zebra "${HOME}/.cache/zebra" \
    && chown -R "${UID}:${GID}" "${HOME}" \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/zebrad /usr/local/bin/zebrad
COPY zebra-crosslink/docker/entrypoint.sh /usr/local/bin/entrypoint.sh
COPY zebra-crosslink/docker/zebrad-rpc.toml /etc/zebra/zebrad.toml

EXPOSE 8230 8232 8233 8080

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["zebrad", "start"]
