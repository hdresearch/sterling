# ── Build stage ────────────────────────────────────────────────────────────────
FROM --platform=linux/amd64 rust:1.87-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies: copy only manifests first
COPY Cargo.toml Cargo.lock ./
# llm_proxy is a workspace member — we need the workspace root Cargo.toml
# When building from repo root: docker build -f crates/llm_proxy/Dockerfile .
COPY crates/llm_proxy/Cargo.toml crates/llm_proxy/Cargo.toml
COPY crates/billing/Cargo.toml crates/billing/Cargo.toml

# Dummy mains/libs so cargo can resolve deps
RUN mkdir -p crates/llm_proxy/src && echo "fn main() {}" > crates/llm_proxy/src/main.rs
RUN mkdir -p crates/billing/src && echo "" > crates/billing/src/lib.rs
RUN cargo build --release -p llm_proxy 2>/dev/null || true

# Now copy real source
COPY crates/llm_proxy crates/llm_proxy
COPY crates/billing crates/billing
RUN touch crates/llm_proxy/src/main.rs  # bust the cache
RUN cargo build --release -p llm_proxy

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM --platform=linux/amd64 debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

# RDS CA bundle — needed for TLS connections to AWS RDS
RUN curl -sS https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem \
      -o /usr/local/share/ca-certificates/rds-ca-bundle.crt && \
    update-ca-certificates

COPY --from=builder /build/target/release/llm_proxy /usr/local/bin/llm_proxy

# Config is mounted or baked in at deploy time
COPY crates/llm_proxy/llm_proxy.production.toml /etc/llm_proxy/llm_proxy.toml

EXPOSE 8090

ENTRYPOINT ["/usr/local/bin/llm_proxy", "/etc/llm_proxy/llm_proxy.toml"]
