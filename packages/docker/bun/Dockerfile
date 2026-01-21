FROM oven/bun:1.1-slim

WORKDIR /app

COPY iii ./iii

EXPOSE 3111
EXPOSE 3112

CMD ["./iii", "--config", "config.yaml"]