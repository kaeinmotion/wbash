# Multi-stage build for wxec MCP server
FROM rust:1.84-slim-bookworm AS builder

WORKDIR /usr/src/wxec

# Install build dependencies for tree-sitter C bindings
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

COPY . .

RUN cargo build --release -p wxec-cli

# Runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/wxec/target/release/wxec /usr/local/bin/wxec
COPY --from=builder /usr/src/wxec/target/release/w /usr/local/bin/w

WORKDIR /workspace

ENTRYPOINT ["wxec"]
