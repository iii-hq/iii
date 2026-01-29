# =============================================================================
# III Engine - Production Dockerfile (uses pre-built release binaries)
# =============================================================================
#
# Downloads pre-built binaries from GitHub releases instead of compiling.
# This ensures consistency with release artifacts and faster builds.
#
# Build with version:
#   docker build --build-arg VERSION=v0.2.0 -t iii:v0.2.0 .
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

ARG VERSION=latest

# -----------------------------------------------------------------------------
# Stage 1: Download binary from GitHub releases
# -----------------------------------------------------------------------------
FROM alpine:3.21 AS downloader

ARG VERSION
ARG TARGETARCH

RUN apk add --no-cache curl tar

WORKDIR /download

RUN set -ex; \
    case "${TARGETARCH}" in \
        amd64) RUST_TARGET="x86_64-unknown-linux-musl" ;; \
        arm64) RUST_TARGET="aarch64-unknown-linux-gnu" ;; \
        *) echo "Unsupported architecture: ${TARGETARCH}" && exit 1 ;; \
    esac; \
    DOWNLOAD_URL="https://github.com/MotiaDev/iii-engine/releases/download/${VERSION}/iii-${RUST_TARGET}.tar.gz"; \
    echo "Downloading from: ${DOWNLOAD_URL}"; \
    curl -fsSL "${DOWNLOAD_URL}" | tar xz; \
    chmod 550 iii

# -----------------------------------------------------------------------------
# Stage 2: Runtime (Alpine for amd64/musl, Debian for arm64/gnu)
# -----------------------------------------------------------------------------
FROM alpine:3.21 AS runtime-amd64
RUN apk add --no-cache ca-certificates curl

FROM debian:bookworm-slim AS runtime-arm64
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# -----------------------------------------------------------------------------
# Stage 3: Final image (select runtime based on architecture)
# -----------------------------------------------------------------------------
ARG TARGETARCH
FROM runtime-${TARGETARCH} AS runtime

# Create non-root user
RUN adduser --system --no-create-home --shell /sbin/nologin iii

WORKDIR /app

# Copy binary from downloader
COPY --from=downloader --chown=iii:iii /download/iii /app/iii

USER iii

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -sf http://localhost:3111/health || curl -sf http://localhost:3111/ || /app/iii --version

# Engine ports
# 49134 - WebSocket server (worker connections)
# 3111  - REST API
# 3112  - Streams API
# 9464  - Prometheus metrics
EXPOSE 49134 3111 3112 9464

ENTRYPOINT ["/app/iii"]
CMD ["--config", "/app/config.yaml"]

# Labels
LABEL org.opencontainers.image.title="III Engine" \
      org.opencontainers.image.description="WebSocket-based process communication engine" \
      org.opencontainers.image.vendor="iiidev" \
      org.opencontainers.image.source="https://github.com/MotiaDev/iii-engine" \
      org.opencontainers.image.documentation="https://github.com/MotiaDev/iii-engine#readme"
