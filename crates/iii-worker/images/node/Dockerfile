FROM node:24-slim AS base

ENV NODE_ENV=development \
    NPM_CONFIG_LOGLEVEL=info

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    && npm install -g tsx \
    && npm cache clean --force \
    && mkdir -p /home/node/work \
    && chown -R node:node /home/node

USER node
WORKDIR /home/node/work
CMD ["tail", "-f", "/dev/null"]

FROM base AS builder

USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    curl \
    build-essential \
    python3 \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    && npm install -g npm@latest \
       typescript \
       ts-node \
       nodemon \
       eslint \
       prettier

USER node

# Keep runtime last so a default build excludes the builder toolchain.
FROM base AS runtime
