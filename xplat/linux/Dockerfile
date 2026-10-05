# Linux validation image for GitRaptor (cross-platform validation stage).
#
# Toolchain only: the repository is never baked in nor bind-mounted from the host. `run-linux.sh`
# streams a Git bundle of the checkout into the container, which clones it onto its own
# filesystem (inotify does not behave on host bind mounts). What it does not cover is in
# xplat/linux/vm.md and docs/architecture/xplat-pendientes.md.
#
# Contents:
# - Rust from rust-toolchain.toml (passed as RUST_TOOLCHAIN by run-linux.sh).
# - Git matrix: the distro Git on PATH, plus the minimum supported (2.38, NFR-07) and the latest
#   release built from source under /opt/git/<version>.
# - strace for GITRAPTOR_EXEC_AUDIT=strace (ADR-GRP-009, Validación 7).
# - Node and pnpm (packageManager in package.json) for the JS side of the workspace.
#
# Tests run as an unprivileged user, as a real install would: SEC-10 rejects Git candidates
# writable by others, and the exec audit must work without root.

FROM ubuntu:24.04

ARG RUST_TOOLCHAIN=1.99
ARG GIT_MIN_VERSION=2.38.5
ARG GIT_LATEST_VERSION=2.56.0
# SHA-256 of the kernel.org tarballs (they match kernel.org's signed sha256sums.asc).
ARG GIT_MIN_SHA256=f1b3d39684545a1954a3ab6e11972405c8eb2cf16ae069e96d0502d97843aea5
ARG GIT_LATEST_SHA256=26c56c296b38c0695b26fa95f475f1d01704d2d38e73465ca30b0b2f5dc789d3
ARG NODE_VERSION=22.23.3
ARG PNPM_VERSION=10.20.0

ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 LC_ALL=C.UTF-8

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl xz-utils pkg-config \
        git strace procps \
        libssl-dev libcurl4-gnutls-dev libexpat1-dev zlib1g-dev gettext \
    && rm -rf /var/lib/apt/lists/*

# Git built from source, one prefix per version. Docs and Tcl/Tk are not needed. Recent Git
# builds its Rust parts by default (NO_RUST opts out), so a throwaway toolchain is installed for
# the build and removed afterwards, to match what upstream ships.
RUN set -eux; \
    export RUSTUP_HOME=/tmp/rustup CARGO_HOME=/tmp/cargo PATH="/tmp/cargo/bin:$PATH"; \
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path \
        --default-toolchain "$RUST_TOOLCHAIN"; \
    for pair in "$GIT_MIN_VERSION:$GIT_MIN_SHA256" "$GIT_LATEST_VERSION:$GIT_LATEST_SHA256"; do \
        v="${pair%%:*}"; \
        curl -fsSLo /tmp/git.tar.xz "https://mirrors.edge.kernel.org/pub/software/scm/git/git-$v.tar.xz"; \
        echo "${pair#*:}  /tmp/git.tar.xz" | sha256sum -c -; \
        tar -xJf /tmp/git.tar.xz -C /tmp && rm /tmp/git.tar.xz; \
        make -C "/tmp/git-$v" -j"$(nproc)" prefix="/opt/git/$v" NO_TCLTK=1 all install >/dev/null; \
        rm -rf "/tmp/git-$v"; \
        "/opt/git/$v/bin/git" version; \
    done; \
    rm -rf /tmp/rustup /tmp/cargo

# Node from the official binary tarball, checked against the release's SHASUMS256.txt; pnpm
# through corepack, pinned like package.json.
RUN set -eux; \
    arch="$(dpkg --print-architecture | sed 's/amd64/x64/')"; \
    tarball="node-v${NODE_VERSION}-linux-${arch}.tar.xz"; \
    cd /tmp; \
    curl -fsSLO "https://nodejs.org/dist/v${NODE_VERSION}/$tarball"; \
    curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt" | grep " $tarball\$" | sha256sum -c -; \
    tar -xJf "$tarball" -C /usr/local --strip-components=1 && rm "$tarball"; \
    corepack enable; \
    node --version

RUN useradd --create-home --uid 1001 --shell /bin/bash raptor \
    && mkdir -p /work /results /cache/target \
    && chown raptor:raptor /work /results /cache /cache/target

USER raptor
ENV HOME=/home/raptor \
    CARGO_HOME=/home/raptor/.cargo \
    RUSTUP_HOME=/home/raptor/.rustup \
    CARGO_TARGET_DIR=/cache/target \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0 \
    PATH=/home/raptor/.cargo/bin:/usr/local/bin:/usr/bin:/bin
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal \
        --default-toolchain "$RUST_TOOLCHAIN" -c clippy -c rustfmt \
    && mkdir -p "$CARGO_HOME/registry" "$CARGO_HOME/git" \
    && corepack prepare "pnpm@$PNPM_VERSION" --activate \
    && rustc --version && pnpm --version

COPY --chmod=755 in-container.sh /usr/local/bin/xplat-in-container
WORKDIR /work
ENTRYPOINT ["/usr/local/bin/xplat-in-container"]
