FROM busybox:1.36.1 AS data-dirs

# /app/config holds the configuration store (one YAML per entry, default
# `./config` with the template's WORKDIR /app). It must exist and be owned by
# the non-root user or the configuration worker fails to create it on boot —
# /app itself is root-owned. Mount a volume over it to persist runtime edits
# across container recreates (the generated docker-compose does).
RUN mkdir -p /app/data /app/config /data && chown -R 65532:65532 /app/data /app/config /data

FROM gcr.io/distroless/cc-debian12:nonroot

ARG TARGETARCH
COPY iii-${TARGETARCH} /app/iii
COPY --from=data-dirs --chown=65532:65532 /app/data /app/data
COPY --from=data-dirs --chown=65532:65532 /app/config /app/config
COPY --from=data-dirs --chown=65532:65532 /data /data

ENV III_EXECUTION_CONTEXT=docker
ENV III_ENV=development

EXPOSE 49134 3111 3112
ENTRYPOINT ["/app/iii"]
CMD ["--config", "/app/config.yaml"]

LABEL org.opencontainers.image.title="III Engine" \
      org.opencontainers.image.vendor="iiidev" \
      org.opencontainers.image.source="https://github.com/iii-hq/iii"
