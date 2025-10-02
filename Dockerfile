FROM ubuntu:24.04

WORKDIR /app

ARG FEATURES=""
ARG PROFILE="release"
ARG CARGO_RISCZERO_VARSION="3.0.3"

LABEL jp.datachain.rust.features="[${FEATURES}]"
LABEL jp.datachain.rust.profile=${PROFILE}

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    gnupg \
    lsb-release \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

COPY . .

# Build bonsai-local
RUN --mount=type=cache,target=${CARGO_HOME}/registry \
    --mount=type=cache,target=/app/target \
    set -eux; \
    FLAGS=""; \
    if [ -n "${FEATURES}" ]; then FLAGS="${FLAGS} --features ${FEATURES}"; fi; \
    cargo build --profile ${PROFILE} ${FLAGS} \
    && cp /app/target/${PROFILE}/bonsai-local /usr/local/bin/bonsai-local

# Install RISC Zero runtime
RUN curl -L https://risczero.com/install | bash
ENV PATH="/root/.risc0/bin:${PATH}"
RUN rzup install rust && rzup install cargo-risczero 3.0.3

# Install Docker CLI only
RUN curl -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /usr/share/keyrings/docker-archive-keyring.gpg && \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker-archive-keyring.gpg] https://download.docker.com/linux/ubuntu \
    $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list > /dev/null && \
    apt-get update && \
    apt-get install -y docker-ce-cli && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -f docker && usermod -aG docker root

VOLUME /var/run/docker.sock

EXPOSE 8080

ENTRYPOINT ["bonsai-local"]