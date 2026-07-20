FROM ubuntu:22.04

ARG RUST_TOOLCHAIN=1.96.0
ARG CARGO_DEB_VERSION=3.7.0

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/opt/rustup/toolchains/${RUST_TOOLCHAIN}-x86_64-unknown-linux-gnu/bin:${PATH}

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        cmake \
        curl \
        git \
        libssl-dev \
        perl \
        pkg-config \
        sudo \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain "${RUST_TOOLCHAIN}" \
        --component clippy,rustfmt \
    && cargo install cargo-deb --version "${CARGO_DEB_VERSION}" --locked

WORKDIR /workspace
