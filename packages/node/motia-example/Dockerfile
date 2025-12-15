FROM oven/bun:1.1-slim

WORKDIR /app

COPY dist/index-production.js .
COPY dist/index-production.js.map .

COPY iii ./iii
COPY config.yaml .

EXPOSE 3111
EXPOSE 31112

CMD ["sh", "-c", "bun run --enable-source-maps index-production.js & ./iii --config config.yaml"]