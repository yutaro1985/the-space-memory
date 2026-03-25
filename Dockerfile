FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY tests/ tests/
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/knowledge-search /usr/local/bin/knowledge-search
ENTRYPOINT ["knowledge-search"]
