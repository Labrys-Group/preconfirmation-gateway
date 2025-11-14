# syntax=docker/dockerfile:1.7-labs
ARG BINARIES="gateway relay proposer spammer local-signer-module beacon-mock"

FROM --platform=${BUILDPLATFORM} rust:1.89-slim-bookworm AS chef
ARG TARGETOS TARGETARCH BUILDPLATFORM TARGET_CRATE BINARY_NAME BINARIES
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    CARGO_NET_GIT_FETCH_WITH_CLI=true
WORKDIR /app
RUN cargo install cargo-chef --locked && rm -rf $CARGO_HOME/registry/

FROM --platform=${BUILDPLATFORM} chef AS planner
ARG TARGETOS TARGETARCH BUILDPLATFORM TARGET_CRATE BINARY_NAME BINARIES
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM --platform=${BUILDPLATFORM} chef AS builder
ARG TARGETOS TARGETARCH BUILDPLATFORM TARGET_CRATE BINARY_NAME BINARIES
ENV DEBIAN_FRONTEND=noninteractive \
    RUSTFLAGS="-C target-cpu=native"
COPY --from=planner /app/recipe.json recipe.json

# Optional cross-compilation environment (only when truly cross compiling)
RUN set -eux; \
    BUILD_VAR_SCRIPT=/tmp/env.sh; \
    if [ "$BUILDPLATFORM" = "linux/amd64" -a "$TARGETARCH" = "arm64" ]; then \
      rustup target add aarch64-unknown-linux-gnu; \
      dpkg --add-architecture arm64; apt-get update; \
      apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu; \
      printf "#!/bin/sh\nexport TARGET=aarch64-unknown-linux-gnu\nexport TARGET_FLAG=--target=aarch64-unknown-linux-gnu\nexport CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=/usr/bin/aarch64-linux-gnu-gcc\n" > ${BUILD_VAR_SCRIPT}; \
    elif [ "$BUILDPLATFORM" = "linux/arm64" -a "$TARGETARCH" = "amd64" ]; then \
      rustup target add x86_64-unknown-linux-gnu; \
      dpkg --add-architecture amd64; apt-get update; \
      apt-get install -y --no-install-recommends gcc-x86-64-linux-gnu; \
      printf "#!/bin/sh\nexport TARGET=x86_64-unknown-linux-gnu\nexport TARGET_FLAG=--target=x86_64-unknown-linux-gnu\nexport CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=/usr/bin/x86_64-linux-gnu-gcc\n" > ${BUILD_VAR_SCRIPT}; \
    fi; \
    if [ -f ${BUILD_VAR_SCRIPT} ]; then . ${BUILD_VAR_SCRIPT}; fi; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
      git curl unzip pkg-config ca-certificates \
      build-essential cmake clang llvm-dev libclang-dev \
      libssl-dev zlib1g-dev protobuf-compiler; \
    rm -rf /var/lib/apt/lists/*;

COPY provisioning/protoc.sh /tmp/protoc.sh
RUN bash /tmp/protoc.sh && rm -f /tmp/protoc.sh

RUN set -eux; if [ -f /tmp/env.sh ]; then . /tmp/env.sh; fi; \
    cargo chef cook ${TARGET_FLAG:-} --release --recipe-path recipe.json

# Build requested binary from the workspace
COPY . .
ENV CARGO_BUILD_JOBS=2
RUN set -eux; if [ -f /tmp/env.sh ]; then . /tmp/env.sh; fi; \
    if [ -n "${TARGET_CRATE:-}" ]; then PKG_FLAG="-p ${TARGET_CRATE}"; else PKG_FLAG=""; fi; \
    cargo build ${TARGET_FLAG:-} --release ${PKG_FLAG} --bins; \
    OUTDIR="target/release"; if [ -n "${TARGET:-}" ]; then OUTDIR="target/${TARGET}/release"; fi; \
    mkdir -p /artifacts; \
    BIN_SELECTION="${BINARIES:-${BINARY_NAME}}"; \
    if [ -z "${BIN_SELECTION}" ]; then \
      echo "No binaries specified via BINARIES or BINARY_NAME" >&2; \
      exit 1; \
    fi; \
    for bin in ${BIN_SELECTION}; do \
      if [ ! -f "${OUTDIR}/${bin}" ]; then \
        echo "Expected binary ${bin} not found in ${OUTDIR}" >&2; \
        exit 1; \
      fi; \
      cp "${OUTDIR}/${bin}" "/artifacts/${bin}"; \
    done

FROM scratch AS output
COPY --from=builder /artifacts/ /


