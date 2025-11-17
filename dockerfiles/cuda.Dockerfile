FROM nvidia/cuda:13.0.1-cudnn-devel-ubuntu24.04 AS builder

WORKDIR /app

ARG FEATURES
ARG PROFILE="release"
ARG CARGO_RISCZERO_VERSION
ARG RISCZERO_GROTH16_VERSION

LABEL jp.datachain.rust.features="[${FEATURES}]" \
      jp.datachain.rust.profile=${PROFILE} \
      jp.datachain.risczero.runtime.version=${CARGO_RISCZERO_VERSION} \
      jp.datachain.risczero.groth16.version=${RISCZERO_GROTH16_VERSION}

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    gnupg \
    build-essential \
    clang \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

COPY . .

# Build bonsai-local
RUN --mount=type=cache,target=${CARGO_HOME}/registry,id=cargo-reg,sharing=locked \
    --mount=type=cache,target=${CARGO_HOME}/git,id=cargo-git,sharing=locked \
    --mount=type=cache,target=/app/target,id=cargo-target \
    cargo build --profile ${PROFILE} --features ${FEATURES} \
    && cp /app/target/${PROFILE}/bonsai-local /usr/local/bin/bonsai-local

FROM nvidia/cuda:13.0.1-cudnn-runtime-ubuntu24.04

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    gnupg \
    lsb-release \
    libssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Install RISC Zero runtime
RUN curl -L https://risczero.com/install | bash
ENV PATH="/root/.risc0/bin:${PATH}"
RUN rzup install rust && \
    rzup install cargo-risczero ${CARGO_RISCZERO_VERSION} && \
    rzup install risc0-groth16 ${RISCZERO_GROTH16_VERSION}

EXPOSE 8080

COPY --from=builder /usr/local/bin/bonsai-local /usr/local/bin/bonsai-local

ENTRYPOINT ["bonsai-local"]