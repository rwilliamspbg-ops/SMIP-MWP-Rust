# Stage 1: Build the Rust application
FROM rust:1.81-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# Stage 2: Minimal runner image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /usr/local/bin
COPY --from=builder /app/target/release/mohawk-node .
COPY --from=builder /app/target/release/benchmark .
COPY --from=builder /app/target/release/bench .
ENTRYPOINT ["mohawk-node"]
