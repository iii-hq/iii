# Build stage for Rust binary
FROM debian:bullseye-slim as rust-builder

WORKDIR /build

# Install Rust and build dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    pkg-config \
    libssl-dev \
    build-essential \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
    apt-get remove -y curl && \
    apt-get autoremove -y && \
    rm -rf /var/lib/apt/lists/*

# Set up Rust environment
ENV PATH="/root/.cargo/bin:${PATH}"

# Copy Cargo files
COPY Cargo.toml Cargo.lock* ./
COPY function-macros ./function-macros
COPY src ./src

# Build natively since we're using platform: linux/amd64 in docker-compose
RUN rustup default stable && \
    cargo build --release && \
    cp target/release/iii /build/iii

# Runtime stage
FROM oven/bun:1.1-slim

WORKDIR /app

# Copy the compiled binary from build stage
COPY --from=rust-builder /build/iii ./iii

# Copy configuration file (must be copied before running the application)
COPY packages/node/motia-example/config.yaml /app/config.yaml

# Copy Node.js application files
COPY packages/node/motia-example/dist/index-production.js /app/index-production.js
COPY packages/node/motia-example/dist/index-production.js.map /app/index-production.js.map

EXPOSE 3111
EXPOSE 31112

CMD ["sh", "-c", "bun run --enable-source-maps index-production.js & ./iii --config config.yaml"]
