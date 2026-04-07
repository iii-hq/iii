FROM node:24-slim

ENV NODE_ENV=development \
    NPM_CONFIG_LOGLEVEL=info

RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    curl \
    ca-certificates \
    build-essential \
    python3 \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /home/node/work \
    && chown -R node:node /home/node \
    && npm install -g npm@latest \
       typescript \
       ts-node \
       nodemon \
       eslint \
       prettier

USER node
WORKDIR /home/node/work

CMD ["tail", "-f", "/dev/null"]
