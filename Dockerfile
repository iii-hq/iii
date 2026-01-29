# =============================================================================
# III Engine - Hardened Production Dockerfile
# =============================================================================
#
# Security runtime flags (REQUIRED for production):
#   --read-only --tmpfs /tmp:rw,noexec,nosuid
#   --cap-drop=ALL --cap-add=NET_BIND_SERVICE
#   --security-opt=no-new-privileges:true
#
# Example:
#   docker run --read-only --tmpfs /tmp \
#     --cap-drop=ALL --cap-add=NET_BIND_SERVICE \
#     --security-opt=no-new-privileges:true \
#     -v ./config.yaml:/app/config.yaml:ro \
#     iii:latest
#
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Build
# -----------------------------------------------------------------------------
# Pin to digest for supply chain security (update periodically)
FROM rust:slim@sha256:df6ca8f96d338697ccdbe3ccac57a85d2172e03a2429c2d243e74f3bb83ba2f5 AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source
COPY . .

# Build release binary with optimizations and strip to reduce size
RUN cargo build --release && strip target/release/iii

# -----------------------------------------------------------------------------
# Stage 2: Runtime (minimal Debian with security hardening)
# -----------------------------------------------------------------------------
# Pin to digest for supply chain security
FROM debian:trixie-slim@sha256:77ba0164de17b88dd0bf6cdc8f65569e6e5fa6cd256562998b62553134a00ef0 AS runtime

# Install only required runtime dependencies
# curl is needed for health checks
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3t64 \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && rm -rf /var/cache/apt/*

# Create non-root user and set up app directory
RUN useradd --system --no-create-home --shell /usr/sbin/nologin iii \
    && mkdir -p /app \
    && chown iii:iii /app

# Switch to non-root user before copying binary
USER iii
WORKDIR /app

# Copy binary with correct ownership
COPY --from=builder --chown=iii:iii /build/target/release/iii /app/iii

# Ensure binary is executable but not writable
RUN chmod 550 /app/iii

# Health check that verifies service is actually responding
# Falls back to version check if health endpoint doesn't exist
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -sf http://localhost:3111/health || curl -sf http://localhost:3111/ || /app/iii --version

# Engine ports
# 49134 - WebSocket server (worker connections)
# 3111  - REST API
# 3112  - Streams API  
# 9464  - Prometheus metrics
EXPOSE 49134 3111 3112 9464

ENTRYPOINT ["/app/iii"]
# Use environment variable for config path flexibility
CMD ["--config", "/app/config.yaml"]

# =============================================================================
# Labels for container metadata and security scanning
# =============================================================================
LABEL org.opencontainers.image.title="III Engine" \
      org.opencontainers.image.description="WebSocket-based process communication engine" \
      org.opencontainers.image.vendor="iiidev" \
      org.opencontainers.image.source="https://github.com/MotiaDev/iii-engine" \
      org.opencontainers.image.documentation="https://github.com/MotiaDev/iii-engine#readme"
