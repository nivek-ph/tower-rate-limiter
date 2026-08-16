# syntax=docker/dockerfile:1

FROM rust:1.96-bookworm AS builder

ARG BENCH_FEATURES=axum,memory,redis,runtime-tokio

WORKDIR /workspace

COPY Cargo.toml Cargo.lock README.md LICENSE-APACHE LICENSE-MIT ./
COPY src ./src
COPY examples ./examples
COPY tests ./tests
COPY benchmarks ./benchmarks

RUN cargo build --locked --release \
    --example benchmark_server \
    --no-default-features \
    --features "${BENCH_FEATURES}"

FROM debian:bookworm-slim

RUN groupadd --system --gid 10001 benchmark \
    && useradd --system --uid 10001 --gid benchmark --no-create-home benchmark

COPY --from=builder /workspace/target/release/examples/benchmark_server /usr/local/bin/benchmark_server

USER benchmark
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/benchmark_server"]
