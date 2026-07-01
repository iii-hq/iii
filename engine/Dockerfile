FROM alpine:3.21 AS prep
RUN mkdir -p /app/data && chown -R 65532:65532 /app/data

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=prep /app/data /app/data

ARG TARGETARCH
COPY iii-${TARGETARCH} /app/iii

ENV III_EXECUTION_CONTEXT=docker
ENV III_ENV=development

EXPOSE 49134 3111 3112 9464
ENTRYPOINT ["/app/iii"]
CMD ["--config", "/app/config.yaml"]

LABEL org.opencontainers.image.title="III Engine" \
      org.opencontainers.image.vendor="iiidev" \
      org.opencontainers.image.source="https://github.com/iii-hq/iii"
