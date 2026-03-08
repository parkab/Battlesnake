# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:latest AS builder

WORKDIR /app

# Copy both manifests so Cargo.lock pins the exact dependency versions
COPY Cargo.toml Cargo.lock ./

# Build a stub binary to cache dependency compilation in its own layer.
# Deleting the stub artifact forces a clean rebuild of the real binary.
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release
RUN rm -f src/main.rs target/release/deps/apex_predator* target/release/apex-predator

# Build the real project
COPY src ./src
RUN cargo build --release

# ── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/apex-predator ./apex-predator

EXPOSE 8080
ENV PORT=8080

CMD ["./apex-predator"]
