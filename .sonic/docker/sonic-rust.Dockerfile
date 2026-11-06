# Local CI image for the Rust lanes. Built by the Sonic before action; not published.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold" \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=mold"

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        cmake \
        curl \
        gcc \
        git \
        make \
        mold \
        pkg-config \
        python3 \
        python3-yaml \
        xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && git config --system --add safe.directory /workspace

# Rust 1.88.0 matches rust-toolchain.toml. nextest is the CI test runner.
# nextest publishes one tarball per arch. `linux` is x86_64; `linux-arm` is aarch64.
RUN arch="$(uname -m)" \
    && case "$arch" in \
        x86_64) nexte_slug=linux ;; \
        aarch64|arm64) nexte_slug=linux-arm ;; \
        *) echo "unsupported architecture: $arch" >&2; exit 1 ;; \
    esac \
    && curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain 1.88.0 --profile minimal \
            --component rustfmt,clippy \
    && curl -LsSf "https://get.nexte.st/0.9.87/${nexte_slug}" \
        | tar zxf - -C /root/.cargo/bin \
    && rustc --version \
    && cargo nextest --version

# ADR-0255: one-shot CI builds. Incremental debug artifacts are huge and
# unused across container runs; line tables keep backtraces useful.
ENV CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=line-tables-only

WORKDIR /workspace
