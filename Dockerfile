FROM rust:slim-bookworm AS chef
WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-chef

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
COPY Cargo.toml Cargo.lock ./
COPY function-macros ./function-macros
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release && strip target/release/iii

FROM gcr.io/distroless/cc-debian12:nonroot

COPY --from=builder /build/target/release/iii /app/iii

EXPOSE 49134 3111 3112 9464
ENTRYPOINT ["/app/iii"]
CMD ["--config", "/app/config.yaml"]

LABEL org.opencontainers.image.title="III Engine" \
      org.opencontainers.image.vendor="iiidev" \
      org.opencontainers.image.source="https://github.com/iii-hq/iii"
