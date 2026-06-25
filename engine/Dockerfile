FROM busybox:1.36.1 AS data-dirs

RUN mkdir -p /app/data /data && chown -R 65532:65532 /app/data /data

FROM gcr.io/distroless/cc-debian12:nonroot

ARG TARGETARCH
COPY iii-${TARGETARCH} /app/iii
COPY --from=data-dirs --chown=65532:65532 /app/data /app/data
COPY --from=data-dirs --chown=65532:65532 /data /data

ENV III_EXECUTION_CONTEXT=docker
ENV III_ENV=development

EXPOSE 49134 3111 3112
ENTRYPOINT ["/app/iii"]
CMD ["--config", "/app/config.yaml"]

LABEL org.opencontainers.image.title="III Engine" \
      org.opencontainers.image.vendor="iiidev" \
      org.opencontainers.image.source="https://github.com/iii-hq/iii"
