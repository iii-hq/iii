ARG USER_NAME="python-user"

FROM python:3.12-slim AS base

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=1 \
    LANG=C.UTF-8 \
    DEBIAN_FRONTEND=noninteractive

ARG USER_NAME
ARG USER_UID="1000"
ARG USER_GID="100"

RUN useradd -m -s /bin/bash -N -u $USER_UID $USER_NAME \
    && mkdir -p /home/$USER_NAME/work \
    && chown -R $USER_NAME:$USER_GID /home/$USER_NAME

USER $USER_NAME
WORKDIR /home/$USER_NAME/work
CMD ["tail", "-f", "/dev/null"]

FROM base AS builder

ARG USER_NAME

USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    curl \
    wget \
    build-essential \
    libssl-dev \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    && pip install --no-cache-dir --upgrade pip setuptools wheel \
    && pip install --no-cache-dir \
        black \
        flake8 \
        mypy \
        pytest \
        pytest-cov \
        requests \
        ipython

USER $USER_NAME

# Keep runtime last so a default build excludes the builder toolchain.
FROM base AS runtime
