FROM rust:1.85-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        cmake \
        libssh2-1-dev \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY Cargo.toml rust-toolchain.toml ./
COPY apps ./apps
COPY crates ./crates

RUN cargo build --release -p tw-einvoice-cli

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssh2-1 \
        libssl3 \
        zlib1g \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /work \
    && chown 65532:65532 /work

COPY --from=builder /src/target/release/tw-einvoice-cli /usr/local/bin/tw-einvoice

ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
WORKDIR /work
USER 65532:65532

ENTRYPOINT ["/usr/local/bin/tw-einvoice"]
CMD ["--help"]
